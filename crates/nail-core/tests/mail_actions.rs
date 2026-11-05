//! Mail actions on conversations (P3, ADR 0164; WP-actions-core):
//! targets resolved per reach (every stored copy), one batch with one
//! undo, the actor's own undo (user, remote session, MCP client), remote
//! sessions within their grant, snooze and junk undone with their side
//! effects, and the demo-multi IMAP account's "Lab move" conversation on
//! the mock server (grouped `UID MOVE`s, undo back to where each was).

#![cfg(test)]

#[expect(dead_code, reason = "each test crate uses part of the shared helpers")]
mod common;
use common::{NIGHTINGALE as SUBJECT, demo_folder, eventually, remote_session};
use nail_core::testing::{TempService, error_of};

use nail_api::agents::{ActionActor, ActionAttribution};
use nail_api::mail_actions::{MailActionKind, MailActionResult};
use nail_api::remote::RemoteAccessPreset;
use nail_api::{
    AccountId, ErrorCode, Folder, FolderId, FolderRole, MailAction, MessageId, MessageTarget,
    MoveTarget, Request, Response, ThreadSummary,
};
use nail_core::demo::{DEMO_ACCOUNT_ID, DemoMail, MULTI_WORK_ID, start_demo_multi_with};
use nail_core::{McpCore, Service, ServiceOptions};
use nail_mcp::CoreAccess;

type Fixture = TempService;

fn setup() -> Fixture {
    let temp = TempService::seeded();
    let service = &temp.service;
    // Trash and Junk folders for deletes and junk (the demo corpus has
    // none).
    for (name, role) in [("Trash", FolderRole::Trash), ("Junk", FolderRole::Junk)] {
        service
            .store()
            .upsert_folder(&Folder {
                id: FolderId::new(format!("demo-{name}")),
                account_id: AccountId::new(DEMO_ACCOUNT_ID),
                parent_id: None,
                name: name.into(),
                path: name.into(),
                role,
                unread: 0,
                total: 0,
            })
            .unwrap();
    }
    temp
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

/// The inbox and its Nightingale conversation row.
async fn inbox_nightingale(f: &Fixture) -> (Folder, ThreadSummary) {
    let inbox = demo_folder(&f.service, FolderRole::Inbox);
    let row = nightingale(f, &inbox).await;
    (inbox, row)
}

/// The whole conversation of `row` as listed in `folder`.
fn whole(row: &ThreadSummary, folder: &Folder) -> Vec<MessageTarget> {
    vec![MessageTarget::conversation(
        row.id.clone(),
        Some(folder.id.clone()),
    )]
}

#[tokio::test]
async fn reach_picks_the_listed_messages_or_the_whole_conversation() {
    let f = setup();
    let (inbox, row) = inbox_nightingale(&f).await;

    let target = whole(&row, &inbox);
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
    let (inbox, row) = inbox_nightingale(&f).await;
    let sent = demo_folder(&f.service, FolderRole::Sent);

    let result = match f
        .service
        .handle(Request::ApplyMailAction {
            targets: whole(&row, &inbox),
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
    let (inbox, row) = inbox_nightingale(&f).await;

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

// ---------------------------------------------------------------------------
// WP-actions-core

async fn apply(f: &Fixture, targets: Vec<MessageTarget>, action: MailAction) -> MailActionResult {
    match f
        .service
        .handle(Request::ApplyMailAction { targets, action })
        .await
    {
        Response::MailActionQueued { result } => result,
        other => panic!("{other:?}"),
    }
}

/// The folders the thread's messages are in now, by folder id.
fn where_are(f: &Fixture, thread: &nail_api::ThreadId) -> Vec<(FolderId, usize)> {
    let store = f.service.store();
    let mut out: Vec<(FolderId, usize)> = Vec::new();
    for folder in store
        .list_folders(&AccountId::new(DEMO_ACCOUNT_ID))
        .unwrap()
    {
        let n = store
            .list_messages(&folder.id, 0, 1000)
            .unwrap()
            .messages
            .iter()
            .filter(|m| m.thread_id.as_ref() == Some(thread))
            .count();
        if n > 0 {
            out.push((folder.id, n));
        }
    }
    out.sort();
    out
}

#[tokio::test]
async fn one_batch_one_undo_and_every_message_back_where_it_was() {
    let f = setup();
    let (inbox, row) = inbox_nightingale(&f).await;
    let archive = demo_folder(&f.service, FolderRole::Archive);

    let before = where_are(&f, &row.id);
    let result = apply(&f, whole(&row, &inbox), MailAction::Archive).await;
    let batch = result.batch_id.clone().expect("archiving is undoable");
    assert!(result.undo_until.is_some());
    assert_eq!(result.op_ids.len(), 4, "one queued move per message");
    let after = where_are(&f, &row.id);
    assert!(
        after.iter().any(|(id, n)| *id == archive.id && *n >= 4),
        "{after:?}"
    );
    assert!(after.iter().all(|(id, _)| *id != inbox.id));

    let undone = f
        .service
        .handle(Request::UndoMailAction {
            batch_id: batch.clone(),
        })
        .await;
    assert!(
        matches!(&undone, Response::Queued { op_ids } if op_ids.len() == 4),
        "{undone:?}"
    );
    assert_eq!(where_are(&f, &row.id), before);
    // Once only; unknown batches look expired.
    let again = f
        .service
        .handle(Request::UndoMailAction { batch_id: batch })
        .await;
    assert_eq!(
        error_of(&again),
        (
            ErrorCode::Conflict,
            Some("mail-action-issue-undone-already".into())
        )
    );
    let unknown = f
        .service
        .handle(Request::UndoMailAction {
            batch_id: "batch-999".into(),
        })
        .await;
    assert_eq!(
        error_of(&unknown),
        (
            ErrorCode::NotFound,
            Some("mail-action-issue-undo-expired".into())
        )
    );
}

#[tokio::test]
async fn delete_reaches_the_replies_in_sent_and_undo_returns_each() {
    let f = setup();
    let (inbox, row) = inbox_nightingale(&f).await;
    let sent = demo_folder(&f.service, FolderRole::Sent);
    let trash = demo_folder(&f.service, FolderRole::Trash);

    let before = where_are(&f, &row.id);
    let result = apply(&f, whole(&row, &inbox), MailAction::Trash).await;
    assert_eq!(result.kind, MailActionKind::Trash);
    assert_eq!(result.message_ids.len(), 7, "four listed, three replies");
    let after = where_are(&f, &row.id);
    assert!(
        after
            .iter()
            .all(|(id, _)| *id != inbox.id && *id != sent.id),
        "{after:?}"
    );
    assert!(after.iter().any(|(id, n)| *id == trash.id && *n >= 7));
    // Delete permanently from Trash: refused while other copies are
    // elsewhere is not the case here, so it is allowed, but not undoable.
    let trash_row = nightingale(&f, &trash).await;
    let resolution = match f
        .service
        .handle(Request::ResolveMailTargets {
            targets: vec![MessageTarget::conversation(
                trash_row.id.clone(),
                Some(trash.id.clone()),
            )],
            action: MailAction::DeletePermanently,
        })
        .await
    {
        Response::MailTargets { resolution } => resolution,
        other => panic!("{other:?}"),
    };
    assert_eq!(resolution.message_ids.len(), 7);
    // Undo the trash first: everything is back where it was.
    let undone = f
        .service
        .handle(Request::UndoMailAction {
            batch_id: result.batch_id.unwrap(),
        })
        .await;
    assert!(matches!(undone, Response::Queued { .. }), "{undone:?}");
    assert_eq!(where_are(&f, &row.id), before);
}

#[tokio::test]
async fn read_state_and_flags_undo_per_message() {
    let f = setup();
    let (inbox, row) = inbox_nightingale(&f).await;

    let target = whole(&row, &inbox);
    let flags = |f: &Fixture| -> Vec<(MessageId, bool, bool)> {
        let (_, messages) = f.service.store().get_thread(&row.id, None).unwrap();
        let mut out: Vec<_> = messages
            .into_iter()
            .map(|m| (m.id, m.flags.seen, m.flags.flagged))
            .collect();
        out.sort();
        out
    };
    let before = flags(&f);
    let unread = apply(&f, target.clone(), MailAction::MarkRead { read: false }).await;
    assert!(flags(&f).iter().all(|(_, seen, _)| !seen));
    // Only the messages that were read are queued.
    let were_read = before.iter().filter(|(_, seen, _)| *seen).count();
    assert_eq!(unread.op_ids.len(), were_read);
    let flagged = apply(&f, target.clone(), MailAction::Flag { flagged: true }).await;
    assert_eq!(flagged.message_ids, vec![row.latest_message_id.clone()]);
    for batch in [flagged.batch_id.unwrap(), unread.batch_id.unwrap()] {
        let undone = f
            .service
            .handle(Request::UndoMailAction { batch_id: batch })
            .await;
        assert!(matches!(undone, Response::Queued { .. }), "{undone:?}");
    }
    assert_eq!(flags(&f), before);
}

#[tokio::test]
async fn snooze_and_junk_undo_their_side_effects() {
    let f = setup();
    let (inbox, row) = inbox_nightingale(&f).await;

    let before = where_are(&f, &row.id);
    let target = whole(&row, &inbox);
    let snoozed = apply(
        &f,
        target.clone(),
        MailAction::Snooze {
            until: "2030-01-01T08:00:00Z".into(),
            wake_folder_id: None,
        },
    )
    .await;
    assert_eq!(f.service.store().list_snoozed(None).unwrap().len(), 4);
    let undone = f
        .service
        .handle(Request::UndoMailAction {
            batch_id: snoozed.batch_id.unwrap(),
        })
        .await;
    assert!(matches!(undone, Response::Queued { .. }), "{undone:?}");
    assert_eq!(f.service.store().list_snoozed(None).unwrap().len(), 0);
    assert_eq!(where_are(&f, &row.id), before);
    let tags = |f: &Fixture| -> Vec<String> {
        let (_, messages) = f.service.store().get_thread(&row.id, None).unwrap();
        messages.into_iter().flat_map(|m| m.tags).collect()
    };
    assert!(!tags(&f).iter().any(|t| t == "$Snoozed"));

    let junk = apply(&f, target, MailAction::Junk { junk: true }).await;
    assert!(where_are(&f, &row.id).iter().all(|(id, _)| *id != inbox.id));
    let undone = f
        .service
        .handle(Request::UndoMailAction {
            batch_id: junk.batch_id.unwrap(),
        })
        .await;
    assert!(matches!(undone, Response::Queued { .. }), "{undone:?}");
    assert_eq!(where_are(&f, &row.id), before);
    let stats = f
        .service
        .store()
        .junk_stats(&AccountId::new(DEMO_ACCOUNT_ID))
        .unwrap();
    assert_eq!(stats.trained_junk, 0, "the training is undone too");
}

#[tokio::test]
async fn an_agents_batch_is_its_own_to_undo() {
    let f = setup();
    let (inbox, row) = inbox_nightingale(&f).await;

    let mcp = McpCore(f.service.clone());
    let by = ActionAttribution::new(ActionActor::mcp_client("claude"), "Claude");
    let result = match mcp
        .handle_write(
            &by,
            Request::ApplyMailAction {
                targets: whole(&row, &inbox),
                action: MailAction::Archive,
            },
        )
        .await
    {
        Response::MailActionQueued { result } => result,
        other => panic!("{other:?}"),
    };
    let batch = result.batch_id.unwrap();
    // The user's undo does not reach an agent's batch …
    let user = f
        .service
        .handle(Request::UndoMailAction {
            batch_id: batch.clone(),
        })
        .await;
    assert_eq!(error_of(&user).0, ErrorCode::NotFound);
    // … nor another agent's; the agent's own does.
    let other = ActionAttribution::new(ActionActor::plugin("some.plugin"), "Plug-in");
    let response = mcp
        .handle_write(
            &other,
            Request::UndoMailAction {
                batch_id: batch.clone(),
            },
        )
        .await;
    assert_eq!(error_of(&response).0, ErrorCode::NotFound);
    let response = mcp
        .handle_write(&by, Request::UndoMailAction { batch_id: batch })
        .await;
    assert!(matches!(response, Response::Queued { .. }), "{response:?}");
}

#[tokio::test]
async fn a_remote_session_acts_on_what_its_grant_shows_and_undoes_its_own() {
    let f = setup();
    let (inbox, row) = inbox_nightingale(&f).await;
    let sent = demo_folder(&f.service, FolderRole::Sent);

    let mut grant = RemoteAccessPreset::NoSend.grant();
    grant.mail.excluded_folders = vec![sent.id.clone()];
    let ctx = remote_session(&f.service, "no-sent", grant).await;
    let target = whole(&row, &inbox);
    // Delete reaches the replies in Sent — but not for this session, and it
    // is not refused for them either.
    let resolved = match f
        .service
        .handle_remote(
            &ctx,
            Request::ResolveMailTargets {
                targets: target.clone(),
                action: MailAction::Trash,
            },
        )
        .await
    {
        Response::MailTargets { resolution } => resolution,
        other => panic!("{other:?}"),
    };
    assert_eq!(resolved.message_ids.len(), 4);
    let result = match f
        .service
        .handle_remote(
            &ctx,
            Request::ApplyMailAction {
                targets: target,
                action: MailAction::Trash,
            },
        )
        .await
    {
        Response::MailActionQueued { result } => result,
        other => panic!("{other:?}"),
    };
    assert_eq!(result.message_ids.len(), 4);
    let in_sent = where_are(&f, &row.id)
        .into_iter()
        .find(|(id, _)| *id == sent.id)
        .map(|(_, n)| n);
    assert_eq!(in_sent, Some(3), "the hidden replies stay");
    let batch = result.batch_id.unwrap();
    // The desktop's user cannot undo the session's batch; the session can.
    let user = f
        .service
        .handle(Request::UndoMailAction {
            batch_id: batch.clone(),
        })
        .await;
    assert_eq!(error_of(&user).0, ErrorCode::NotFound);
    let undone = f
        .service
        .handle_remote(&ctx, Request::UndoMailAction { batch_id: batch })
        .await;
    assert!(matches!(undone, Response::Queued { .. }), "{undone:?}");
    // A move into a folder outside the grant is refused.
    let refused = f
        .service
        .handle_remote(
            &ctx,
            Request::ApplyMailAction {
                targets: vec![MessageTarget::conversation(row.id, Some(inbox.id))],
                action: MailAction::Move {
                    target: MoveTarget::FolderId(sent.id),
                },
            },
        )
        .await;
    assert_eq!(error_of(&refused).0, ErrorCode::Forbidden);
}

// ---------------------------------------------------------------------------
// The demo-multi IMAP account against the mock server

/// Lab-move messages per mailbox on the mock server.
fn on_server(demo: &nail_core::demo::DemoMulti) -> Vec<(String, usize)> {
    let mut out: Vec<(String, usize)> = demo
        .work
        .store()
        .mailboxes()
        .into_iter()
        .map(|m| {
            let n = m
                .messages
                .iter()
                .filter(|s| {
                    String::from_utf8_lossy(&s.raw).contains("Subject: Re: Lab move")
                        || String::from_utf8_lossy(&s.raw).contains("Subject: Lab move")
                })
                .count();
            (m.name, n)
        })
        .filter(|(_, n)| *n > 0)
        .collect();
    out.sort();
    out
}

#[tokio::test]
async fn the_lab_move_conversation_on_the_imap_server() {
    let demo = start_demo_multi_with(DemoMail::WithConversations)
        .await
        .unwrap();
    let data = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    demo.write_config(config.path()).unwrap();
    let service =
        Service::open(demo.service_options(ServiceOptions::new(data.path(), config.path())))
            .unwrap();
    let work_folder = |role: FolderRole| {
        service
            .store()
            .list_folders(&AccountId::new(MULTI_WORK_ID))
            .unwrap()
            .into_iter()
            .find(|f| f.role == role)
    };
    eventually("the work account synced with Sent", || async {
        work_folder(FolderRole::Inbox).is_some() && work_folder(FolderRole::Sent).is_some() && {
            let inbox = work_folder(FolderRole::Inbox).unwrap();
            matches!(
                service.handle(Request::ListThreads { folder_id: inbox.id, offset: 0, limit: 200 }).await,
                Response::Threads { threads, .. }
                    if threads.iter().any(|t| t.subject.contains("Lab move") && t.message_count == 12)
            )
        }
    })
    .await;
    let inbox = work_folder(FolderRole::Inbox).unwrap();
    let row = match service
        .handle(Request::ListThreads {
            folder_id: inbox.id.clone(),
            offset: 0,
            limit: 200,
        })
        .await
    {
        Response::Threads { threads, .. } => threads
            .into_iter()
            .find(|t| t.subject.contains("Lab move"))
            .unwrap(),
        other => panic!("{other:?}"),
    };
    let start = on_server(&demo);
    assert_eq!(
        start,
        [("INBOX".to_owned(), 9), ("Sent".to_owned(), 3)],
        "the demo conversation"
    );
    let target = whole(&row, &inbox);
    let act = |action: MailAction| {
        let (service, target) = (service.clone(), target.clone());
        async move {
            match service
                .handle(Request::ApplyMailAction {
                    targets: target,
                    action,
                })
                .await
            {
                Response::MailActionQueued { result } => result,
                other => panic!("{other:?}"),
            }
        }
    };
    let undo = |batch: nail_api::MailBatchId| {
        let service = service.clone();
        async move {
            let response = service
                .handle(Request::UndoMailAction { batch_id: batch })
                .await;
            assert!(matches!(response, Response::Queued { .. }), "{response:?}");
        }
    };

    // Archive: the nine in the Inbox, with one UID MOVE.
    demo.work.clear_commands();
    let archived = act(MailAction::Archive).await;
    assert_eq!(archived.message_ids.len(), 9);
    eventually("archived on the server", || async {
        on_server(&demo) == [("Archive".to_owned(), 9), ("Sent".to_owned(), 3)]
    })
    .await;
    assert_eq!(
        demo.work.command_count("UID MOVE"),
        1,
        "{:?}",
        demo.work.commands()
    );
    undo(archived.batch_id.unwrap()).await;
    eventually("back in the Inbox", || async { on_server(&demo) == start }).await;

    // Delete: the Inbox's and the user's replies in Sent go to Trash.
    demo.work.clear_commands();
    let trashed = act(MailAction::Trash).await;
    assert_eq!(trashed.message_ids.len(), 12);
    eventually("in Trash on the server", || async {
        on_server(&demo) == [("Trash".to_owned(), 12)]
    })
    .await;
    assert_eq!(
        demo.work.command_count("UID MOVE"),
        2,
        "one per source folder: {:?}",
        demo.work.commands()
    );
    undo(trashed.batch_id.unwrap()).await;
    eventually("each back where it was", || async {
        on_server(&demo) == start
    })
    .await;

    // Read state: one UID STORE for the whole conversation.
    demo.work.clear_commands();
    let unread = act(MailAction::MarkRead { read: false }).await;
    assert!(!unread.op_ids.is_empty(), "{:?}", unread.op_ids);
    eventually("unread on the server", || async {
        demo.work.store().mailboxes().iter().all(|m| {
            m.messages
                .iter()
                .filter(|s| String::from_utf8_lossy(&s.raw).contains("Lab move"))
                .all(|s| !s.has_flag("\\Seen"))
        })
    })
    .await;
    assert!(
        demo.work.command_count("UID STORE") <= 2,
        "one per folder: {:?}",
        demo.work.commands()
    );
}

// ---------------------------------------------------------------------------
// P3 review fixes

/// Trash never deletes for good: Delete on a conversation listed in Trash
/// leaves its Trash copies (only Delete permanently removes them, with its
/// own scope and question).
#[tokio::test]
async fn trash_on_a_conversation_in_trash_never_deletes_for_good() {
    let f = setup();
    let (inbox, row) = inbox_nightingale(&f).await;
    let trash = demo_folder(&f.service, FolderRole::Trash);

    apply(&f, whole(&row, &inbox), MailAction::Trash).await;
    let in_trash = where_are(&f, &row.id);
    assert_eq!(in_trash, vec![(trash.id.clone(), 7)]);
    let trash_row = nightingale(&f, &trash).await;
    let again = f
        .service
        .handle(Request::ApplyMailAction {
            targets: vec![MessageTarget::conversation(
                trash_row.id.clone(),
                Some(trash.id.clone()),
            )],
            action: MailAction::Trash,
        })
        .await;
    assert_eq!(
        error_of(&again),
        (
            ErrorCode::InvalidRequest,
            Some("mail-action-issue-no-messages".into())
        )
    );
    // A single message in Trash too.
    let one = f
        .service
        .handle(Request::ApplyMailAction {
            targets: vec![MessageTarget::message(trash_row.latest_message_id.clone())],
            action: MailAction::Trash,
        })
        .await;
    assert_eq!(error_of(&one).0, ErrorCode::InvalidRequest);
    assert_eq!(where_are(&f, &row.id), in_trash, "nothing was expunged");

    // A remote session without the permanent-delete scope cannot expunge
    // through the older DeleteMessages either.
    let mut grant = RemoteAccessPreset::NoSend.grant();
    grant
        .mail
        .scopes
        .retain(|s| *s != nail_api::mcp::McpScope::PermanentDelete);
    let ctx = remote_session(&f.service, "organize-only", grant).await;
    let refused = f
        .service
        .handle_remote(
            &ctx,
            Request::DeleteMessages {
                message_ids: vec![trash_row.latest_message_id.clone()],
                permanent: false,
            },
        )
        .await;
    assert_eq!(error_of(&refused).0, ErrorCode::Forbidden);
    assert_eq!(where_are(&f, &row.id), in_trash);
}

/// Delete is Gmail's: the whole thread goes to Trash, also its archived
/// messages; restoring it from Trash to the Inbox puts the user's replies
/// back into Sent, not into the Inbox.
#[tokio::test]
async fn delete_takes_the_whole_thread_and_a_restore_returns_replies_to_sent() {
    let f = setup();
    let (inbox, row) = inbox_nightingale(&f).await;
    let archive = demo_folder(&f.service, FolderRole::Archive);
    let sent = demo_folder(&f.service, FolderRole::Sent);
    let trash = demo_folder(&f.service, FolderRole::Trash);

    // The account's configured address is not the one its mail is sent
    // from: the addresses in Sent still tell which messages are the user's.
    let store = f.service.store();
    let mut account = store.get_account(&AccountId::new(DEMO_ACCOUNT_ID)).unwrap();
    account.email = "someone-else@example.org".into();
    store.upsert_account(&account).unwrap();
    // One message of the conversation was archived on its own earlier.
    let (_, messages) = f.service.store().get_thread(&row.id, None).unwrap();
    let archived = messages
        .iter()
        .find(|m| m.folder_id == inbox.id)
        .unwrap()
        .id
        .clone();
    apply(
        &f,
        vec![MessageTarget::message(archived.clone())],
        MailAction::Archive,
    )
    .await;
    let before = where_are(&f, &row.id);
    assert!(before.contains(&(archive.id.clone(), 1)), "{before:?}");
    let deleted = apply(&f, whole(&row, &inbox), MailAction::Trash).await;
    assert_eq!(deleted.message_ids.len(), 7, "inbox, archive and Sent");
    assert_eq!(where_are(&f, &row.id), vec![(trash.id.clone(), 7)]);

    // Restored without the undo: Move to Inbox from Trash.
    let trash_row = nightingale(&f, &trash).await;
    apply(
        &f,
        vec![MessageTarget::conversation(
            trash_row.id.clone(),
            Some(trash.id.clone()),
        )],
        MailAction::Move {
            target: MoveTarget::Role(FolderRole::Inbox),
        },
    )
    .await;
    let restored = where_are(&f, &row.id);
    assert_eq!(
        restored,
        {
            let mut v = vec![(inbox.id.clone(), 4), (sent.id.clone(), 3)];
            v.sort();
            v
        },
        "the user's replies go back to Sent"
    );
}
