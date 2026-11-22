//! Mail actions on conversations (P3, ADR 0164; WP-actions-core):
//! targets resolved per reach (every stored copy), one batch with one
//! undo, the actor's own undo (user, remote session, MCP client), remote
//! sessions within their grant, snooze and junk undone with their side
//! effects, and the demo-multi IMAP account's "Lab move" conversation on
//! the mock server (grouped `UID MOVE`s, undo back to where each was).

#![cfg(test)]

#[expect(dead_code, reason = "each test crate uses part of the shared helpers")]
mod common;
use common::{
    BULK_ACTION_MESSAGES as BULK, NIGHTINGALE as SUBJECT, demo_folder, eventually, remote_session,
};
use nail_core::testing::{TempService, error_of};

use nail_api::agents::{ActionActor, ActionAttribution};
use nail_api::mail_actions::{
    MAX_MAIL_ACTION_TARGETS, MailActionKind, MailActionResult, MailList, MailTargetResolution,
};
use nail_api::remote::RemoteAccessPreset;
use nail_api::{
    AccountId, ErrorCode, Folder, FolderId, FolderRole, MailAction, MessageId, MessageTarget,
    MoveTarget, Request, Response, SearchQuery, ThreadSummary,
};
use nail_core::demo::{DEMO_ACCOUNT_ID, DemoMail, MULTI_WORK_ID, start_demo_multi_with};
use nail_core::{McpCore, Service, ServiceOptions};
use nail_mcp::CoreAccess;
use nail_remote_auth::RemoteSessionCtx;

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

/// Undoes `batch` as the user; the undo must be queued.
async fn undo(f: &Fixture, batch: nail_api::MailBatchId) {
    let undone = f
        .service
        .handle(Request::UndoMailAction { batch_id: batch })
        .await;
    assert!(matches!(undone, Response::Queued { .. }), "{undone:?}");
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
    undo(&f, result.batch_id.unwrap()).await;
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
    undo(&f, snoozed.batch_id.unwrap()).await;
    assert_eq!(f.service.store().list_snoozed(None).unwrap().len(), 0);
    assert_eq!(where_are(&f, &row.id), before);
    let tags = |f: &Fixture| -> Vec<String> {
        let (_, messages) = f.service.store().get_thread(&row.id, None).unwrap();
        messages.into_iter().flat_map(|m| m.tags).collect()
    };
    assert!(!tags(&f).iter().any(|t| t == "$Snoozed"));

    let junk = apply(&f, target, MailAction::Junk { junk: true }).await;
    assert!(where_are(&f, &row.id).iter().all(|(id, _)| *id != inbox.id));
    undo(&f, junk.batch_id.unwrap()).await;
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

/// The Sent folder and a remote session whose grant leaves it out.
async fn session_without_sent(f: &Fixture, name: &str) -> (Folder, RemoteSessionCtx) {
    let sent = demo_folder(&f.service, FolderRole::Sent);
    let mut grant = RemoteAccessPreset::NoSend.grant();
    grant.mail.excluded_folders = vec![sent.id.clone()];
    let ctx = remote_session(&f.service, name, grant).await;
    (sent, ctx)
}

#[tokio::test]
async fn a_remote_session_acts_on_what_its_grant_shows_and_undoes_its_own() {
    let f = setup();
    let (inbox, row) = inbox_nightingale(&f).await;
    let (sent, ctx) = session_without_sent(&f, "no-sent").await;
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

// ---------------------------------------------------------------------------
// R5 (ADR 0235): a whole list except some rows

async fn resolve(
    f: &Fixture,
    targets: Vec<MessageTarget>,
    action: MailAction,
) -> MailTargetResolution {
    match f
        .service
        .handle(Request::ResolveMailTargets { targets, action })
        .await
    {
        Response::MailTargets { resolution } => resolution,
        other => panic!("{other:?}"),
    }
}

/// Every conversation row of `folder` as the target it stands for.
async fn row_targets(f: &Fixture, folder: &Folder) -> Vec<MessageTarget> {
    rows_of(f, folder)
        .await
        .into_iter()
        .map(|t| MessageTarget::conversation(t.id, Some(folder.id.clone())))
        .collect()
}

/// Every conversation row of `folder`.
async fn rows_of(f: &Fixture, folder: &Folder) -> Vec<ThreadSummary> {
    match f
        .service
        .handle(Request::ListThreads {
            folder_id: folder.id.clone(),
            offset: 0,
            limit: 1000,
        })
        .await
    {
        Response::Threads { threads, .. } => threads,
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn a_list_target_is_every_row_but_the_deselected_ones() {
    let f = setup();
    let (inbox, row) = inbox_nightingale(&f).await;
    let read = MailAction::MarkRead { read: false };
    let rows = row_targets(&f, &inbox).await;
    let one_by_one = resolve(&f, rows.clone(), read.clone()).await;
    let conversations = MailList::Conversations {
        folder_id: inbox.id.clone(),
    };
    let all = resolve(
        &f,
        vec![MessageTarget::list(conversations.clone(), vec![])],
        read.clone(),
    )
    .await;
    assert_eq!(all.conversations, one_by_one.conversations);
    assert_eq!(all.messages, one_by_one.messages);
    assert_eq!(
        usize::try_from(one_by_one.messages).unwrap(),
        one_by_one.message_ids.len()
    );
    assert_eq!(
        all.message_ids,
        Vec::<MessageId>::new(),
        "a list answers with the count"
    );
    assert_eq!(all.account_ids, one_by_one.account_ids);

    // Deselecting the Nightingale row leaves out its seven messages.
    let but_one = resolve(
        &f,
        vec![MessageTarget::list(conversations, whole(&row, &inbox))],
        read,
    )
    .await;
    assert_eq!(but_one.conversations, all.conversations - 1);
    assert_eq!(but_one.messages, all.messages - 7);

    // A list without conversations: one row per message of the folder.
    let messages = resolve(
        &f,
        vec![MessageTarget::list(
            MailList::Messages {
                folder_id: inbox.id.clone(),
            },
            vec![],
        )],
        MailAction::Archive,
    )
    .await;
    assert_eq!(messages.conversations, 0);
    assert_eq!(messages.messages, inbox_total(&f, &inbox));
}

fn inbox_total(f: &Fixture, inbox: &Folder) -> u32 {
    f.service
        .store()
        .list_messages(&inbox.id, 0, 1)
        .unwrap()
        .total
}

/// The hits of `query`.
async fn hits_of(f: &Fixture, query: &SearchQuery) -> Vec<nail_api::SearchHit> {
    match f
        .service
        .handle(Request::Search {
            query: query.clone(),
        })
        .await
    {
        Response::SearchResults { hits, .. } => hits,
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn a_search_list_is_every_local_hit() {
    let f = setup();
    f.service.rebuild_search_index().unwrap();
    let inbox = demo_folder(&f.service, FolderRole::Inbox);
    let mut query = SearchQuery::new("subject:Nightingale", 1000);
    query.folder_id = Some(inbox.id.clone());
    let hits = hits_of(&f, &query).await;
    assert!(hits.len() > 1, "{hits:?}");
    // The list's own page size does not matter: every hit is a row.
    query.limit = 1;
    let list = |conversations| {
        vec![MessageTarget::list(
            MailList::Search {
                query: query.clone(),
                conversations,
            },
            vec![],
        )]
    };
    let as_messages = resolve(&f, list(false), MailAction::Archive).await;
    assert_eq!(usize::try_from(as_messages.messages).unwrap(), hits.len());
    assert_eq!(as_messages.conversations, 0);
    // As conversations: each hit stands for its conversation without a
    // folder, as the row acts on its own.
    let read = MailAction::MarkRead { read: true };
    let rows = hits
        .iter()
        .map(|h| MessageTarget::conversation(h.message.thread_id.clone().unwrap(), None))
        .collect();
    let one_by_one = resolve(&f, rows, read.clone()).await;
    let as_conversations = resolve(&f, list(true), read).await;
    assert_eq!(as_conversations.messages, one_by_one.messages);
    assert_eq!(as_conversations.conversations, one_by_one.conversations);
}

#[tokio::test]
async fn rows_named_one_by_one_are_capped_and_lists_never_nest() {
    let f = setup();
    let inbox = demo_folder(&f.service, FolderRole::Inbox);
    let list = MessageTarget::list(
        MailList::Conversations {
            folder_id: inbox.id.clone(),
        },
        vec![],
    );
    let issue_of = |response: Response| match response {
        Response::Error { localized, .. } => localized.map(|l| l.id),
        other => panic!("{other:?}"),
    };
    let too_many = (0..=MAX_MAIL_ACTION_TARGETS)
        .map(|n| MessageTarget::message(format!("m{n}")))
        .collect::<Vec<_>>();
    let MessageTarget::List(mut nested) = list.clone() else {
        unreachable!()
    };
    nested.except = too_many;
    assert_eq!(
        issue_of(
            f.service
                .handle(Request::ResolveMailTargets {
                    targets: vec![MessageTarget::List(nested)],
                    action: MailAction::Archive,
                })
                .await
        ),
        Some("mail-action-issue-too-many-targets".to_owned()),
        "exceptions count as rows named one by one"
    );
    assert_eq!(
        issue_of(
            f.service
                .handle(Request::ApplyMailAction {
                    targets: vec![MessageTarget::list(
                        MailList::Messages {
                            folder_id: inbox.id
                        },
                        vec![list]
                    )],
                    action: MailAction::Archive,
                })
                .await
        ),
        Some("mail-action-issue-nested-list".to_owned())
    );
}

#[tokio::test]
async fn archiving_a_whole_list_is_one_batch_with_one_undo() {
    let f = setup();
    let (inbox, row) = inbox_nightingale(&f).await;
    let before = inbox_total(&f, &inbox);
    let result = apply(
        &f,
        vec![MessageTarget::list(
            MailList::Conversations {
                folder_id: inbox.id.clone(),
            },
            whole(&row, &inbox),
        )],
        MailAction::Archive,
    )
    .await;
    assert_eq!(inbox_total(&f, &inbox), 4, "only the deselected row stays");
    assert_eq!(u32::try_from(result.message_ids.len()).unwrap(), before - 4);
    undo(&f, result.batch_id.unwrap()).await;
    assert_eq!(inbox_total(&f, &inbox), before);
}

// ---------------------------------------------------------------------------
// R5 core (ADR 0235): the as_of mark, every list kind, parts with one undo

/// A list target of `list` cut at `as_of`.
fn list_at(list: MailList, except: Vec<MessageTarget>, as_of: Option<String>) -> MessageTarget {
    let MessageTarget::List(mut target) = MessageTarget::list(list, except) else {
        unreachable!()
    };
    target.as_of = as_of;
    MessageTarget::List(target)
}

/// Stores `n` plain messages `<prefix>-<i>` in `folder` of the demo account.
fn store_messages(f: &Fixture, folder: &FolderId, prefix: &str, n: usize) -> Vec<MessageId> {
    let messages: Vec<nail_store::StoredMessage> = (0..n)
        .map(|i| {
            nail_store::StoredMessage::new(nail_api::MessageSummary {
                date: format!("2026-10-{:02}T10:00:00Z", 1 + i % 28),
                ..nail_api::testing::summary(
                    &format!("{prefix}-{i}"),
                    DEMO_ACCOUNT_ID,
                    folder.as_str(),
                )
            })
        })
        .collect();
    f.service.store().upsert_messages(&messages).unwrap();
    messages.into_iter().map(|m| m.summary.id).collect()
}

fn folder_total(f: &Fixture, folder: &FolderId) -> u32 {
    f.service.store().list_messages(folder, 0, 1).unwrap().total
}

#[tokio::test]
async fn the_as_of_mark_keeps_mail_that_arrived_after_the_count_out() {
    let f = setup();
    let inbox = demo_folder(&f.service, FolderRole::Inbox);
    let before = folder_total(&f, &inbox.id);
    for (round, list) in [
        MailList::Messages {
            folder_id: inbox.id.clone(),
        },
        MailList::Conversations {
            folder_id: inbox.id.clone(),
        },
    ]
    .into_iter()
    .enumerate()
    {
        let counted = resolve(
            &f,
            vec![list_at(list.clone(), vec![], None)],
            MailAction::Trash,
        )
        .await;
        let mark = counted.as_of.clone().expect("a list answers with a mark");
        // Delete on conversations also takes the user's replies in Sent.
        let reached = counted.messages;
        assert!(reached >= before, "{reached} < {before}");
        // Mail that arrives after the confirmation is not counted with the
        // mark, and not acted on with it.
        let late = store_messages(&f, &inbox.id, &format!("late-{round}"), 1);
        let again = resolve(
            &f,
            vec![list_at(list.clone(), vec![], Some(mark.clone()))],
            MailAction::Trash,
        )
        .await;
        assert_eq!(again.messages, reached);
        assert_eq!(again.as_of.as_ref(), Some(&mark), "the list's own mark");
        let fresh = resolve(
            &f,
            vec![list_at(list.clone(), vec![], None)],
            MailAction::Trash,
        )
        .await;
        assert_eq!(fresh.messages, reached + 1, "without the mark it counts");
        let result = apply(
            &f,
            vec![list_at(list, vec![], Some(mark))],
            MailAction::Trash,
        )
        .await;
        assert_eq!(u32::try_from(result.message_ids.len()).unwrap(), reached);
        assert!(!result.message_ids.contains(&late[0]));
        assert_eq!(folder_total(&f, &inbox.id), 1, "only the late message");
        undo(&f, result.batch_id.unwrap()).await;
        assert_eq!(folder_total(&f, &inbox.id), before + 1);
        // The late message leaves again for the next round.
        let gone = apply(
            &f,
            vec![MessageTarget::message(late[0].clone())],
            MailAction::Trash,
        )
        .await;
        assert_eq!(gone.message_ids, late);
        assert_eq!(folder_total(&f, &inbox.id), before);
    }
    // A mark the core never made is refused.
    let bad = f
        .service
        .handle(Request::ApplyMailAction {
            targets: vec![list_at(
                MailList::Messages {
                    folder_id: inbox.id.clone(),
                },
                vec![],
                Some("yesterday".into()),
            )],
            action: MailAction::Archive,
        })
        .await;
    assert_eq!(error_of(&bad).0, ErrorCode::InvalidRequest);
    // Without a list there are ids and no mark.
    let rows = row_targets(&f, &inbox).await;
    let named = resolve(&f, rows, MailAction::Trash).await;
    assert_eq!(named.as_of, None);
    assert_eq!(
        usize::try_from(named.messages).unwrap(),
        named.message_ids.len()
    );
}

#[tokio::test]
async fn the_unified_inbox_and_a_message_list_resolve_like_their_rows() {
    let f = setup();
    let inbox = demo_folder(&f.service, FolderRole::Inbox);
    let read = MailAction::MarkRead { read: false };
    let in_folder = resolve(
        &f,
        vec![MessageTarget::list(
            MailList::Conversations {
                folder_id: inbox.id.clone(),
            },
            vec![],
        )],
        read.clone(),
    )
    .await;
    let unified = FolderId::new("all/inbox");
    let rows: Vec<MessageTarget> = match f
        .service
        .handle(Request::ListThreads {
            folder_id: unified.clone(),
            offset: 0,
            limit: 1000,
        })
        .await
    {
        Response::Threads { threads, .. } => threads
            .into_iter()
            .map(|t| MessageTarget::conversation(t.id, Some(unified.clone())))
            .collect(),
        other => panic!("{other:?}"),
    };
    let one_by_one = resolve(&f, rows.clone(), read.clone()).await;
    let all = resolve(
        &f,
        vec![MessageTarget::list(
            MailList::Conversations {
                folder_id: unified.clone(),
            },
            vec![],
        )],
        read.clone(),
    )
    .await;
    assert_eq!(
        (all.messages, all.conversations),
        (one_by_one.messages, one_by_one.conversations)
    );
    assert_eq!(
        (all.messages, all.conversations),
        (in_folder.messages, in_folder.conversations),
        "the demo has one account"
    );
    // Deselected rows of the unified inbox are left out.
    let but_two = resolve(
        &f,
        vec![MessageTarget::list(
            MailList::Conversations { folder_id: unified },
            rows[..2].to_vec(),
        )],
        read,
    )
    .await;
    assert_eq!(but_two.conversations, all.conversations - 2);

    // A message list: one row per message, deselected messages left out.
    let messages = f.service.store().list_messages(&inbox.id, 0, 3).unwrap();
    let except: Vec<MessageTarget> = messages
        .messages
        .iter()
        .map(|m| MessageTarget::message(m.id.clone()))
        .collect();
    let list = resolve(
        &f,
        vec![MessageTarget::list(
            MailList::Messages {
                folder_id: inbox.id.clone(),
            },
            except,
        )],
        MailAction::Flag { flagged: true },
    )
    .await;
    assert_eq!(list.messages, messages.total - 3);
    assert_eq!(list.conversations, 0);
}

#[tokio::test]
async fn a_filter_chip_list_is_its_search_minus_the_deselected_hits() {
    let f = setup();
    f.service.rebuild_search_index().unwrap();
    let inbox = demo_folder(&f.service, FolderRole::Inbox);
    // The "Unread" chip on the Inbox: its term within the folder.
    let mut query = SearchQuery::new("is:unread", 1000);
    query.folder_id = Some(inbox.id.clone());
    let hits = hits_of(&f, &query).await;
    assert!(hits.len() > 2, "{hits:?}");
    let chip = |conversations, except| {
        vec![MessageTarget::list(
            MailList::Search {
                query: SearchQuery {
                    limit: 50,
                    ..query.clone()
                },
                conversations,
            },
            except,
        )]
    };
    let archive = MailAction::Archive;
    let all = resolve(&f, chip(false, vec![]), archive.clone()).await;
    assert_eq!(usize::try_from(all.messages).unwrap(), hits.len());
    let first = MessageTarget::message(hits[0].message.id.clone());
    let but_one = resolve(&f, chip(false, vec![first]), archive.clone()).await;
    assert_eq!(but_one.messages, all.messages - 1);
    // As conversations (a list with conversations): each hit stands for
    // its conversation without a folder; a deselected conversation row
    // leaves out all of its messages.
    let rows: Vec<MessageTarget> = hits
        .iter()
        .map(|h| MessageTarget::conversation(h.message.thread_id.clone().unwrap(), None))
        .collect();
    let one_by_one = resolve(&f, rows.clone(), archive.clone()).await;
    let threads = resolve(&f, chip(true, vec![]), archive.clone()).await;
    assert_eq!(
        (threads.messages, threads.conversations),
        (one_by_one.messages, one_by_one.conversations)
    );
    let first_row = resolve(&f, vec![rows[0].clone()], archive.clone()).await;
    let without_first = resolve(&f, chip(true, vec![rows[0].clone()]), archive).await;
    assert_eq!(
        without_first.messages,
        threads.messages - first_row.messages
    );
}

#[tokio::test]
async fn a_remote_sessions_list_stays_within_its_grant() {
    let f = setup();
    let inbox = demo_folder(&f.service, FolderRole::Inbox);
    let (sent, ctx) = session_without_sent(&f, "list").await;
    let list = |folder: &Folder| {
        vec![MessageTarget::list(
            MailList::Conversations {
                folder_id: folder.id.clone(),
            },
            vec![],
        )]
    };
    let ask = |targets: Vec<MessageTarget>| {
        f.service.handle_remote(
            &ctx,
            Request::ResolveMailTargets {
                targets,
                action: MailAction::Trash,
            },
        )
    };
    let session = match ask(list(&inbox)).await {
        Response::MailTargets { resolution } => resolution,
        other => panic!("{other:?}"),
    };
    let user = resolve(&f, list(&inbox), MailAction::Trash).await;
    assert!(
        session.messages < user.messages,
        "Delete takes the replies in Sent, but not the session's: {} vs {}",
        session.messages,
        user.messages
    );
    assert_eq!(
        session.messages,
        folder_total(&f, &inbox.id),
        "the Inbox only"
    );
    assert_eq!(error_of(&ask(list(&sent)).await).0, ErrorCode::Forbidden);
    let sent_before = folder_total(&f, &sent.id);
    let result = match f
        .service
        .handle_remote(
            &ctx,
            Request::ApplyMailAction {
                targets: list(&inbox),
                action: MailAction::Trash,
            },
        )
        .await
    {
        Response::MailActionQueued { result } => result,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        u32::try_from(result.message_ids.len()).unwrap(),
        session.messages
    );
    assert_eq!(folder_total(&f, &inbox.id), 0);
    assert_eq!(
        folder_total(&f, &sent.id),
        sent_before,
        "the hidden replies stay"
    );
}

/// How many events of each kind arrived, and whether the receiver fell
/// behind.
fn count_events(
    rx: &mut tokio::sync::broadcast::Receiver<nail_api::Event>,
) -> (std::collections::BTreeMap<String, usize>, bool) {
    let mut kinds = std::collections::BTreeMap::new();
    let mut lagged = false;
    loop {
        match rx.try_recv() {
            Ok(event) => {
                let kind = serde_json::to_value(&event).unwrap()["type"]
                    .as_str()
                    .unwrap()
                    .to_owned();
                *kinds.entry(kind).or_insert(0) += 1;
            }
            Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => lagged = true,
            Err(_) => return (kinds, lagged),
        }
    }
}

#[tokio::test]
async fn a_list_of_25000_messages_moves_in_parts_with_one_undo() {
    let f = setup();
    let bulk = FolderId::new("demo-Bulk");
    let trash = demo_folder(&f.service, FolderRole::Trash);
    f.service
        .store()
        .upsert_folder(&Folder {
            id: bulk.clone(),
            account_id: AccountId::new(DEMO_ACCOUNT_ID),
            parent_id: None,
            name: "Bulk".into(),
            path: "Bulk".into(),
            role: FolderRole::Other,
            unread: 0,
            total: 0,
        })
        .unwrap();
    store_messages(&f, &bulk, "bulk", BULK);
    let list = vec![MessageTarget::list(
        MailList::Messages {
            folder_id: bulk.clone(),
        },
        vec![],
    )];
    let started = std::time::Instant::now();
    let counted = resolve(&f, list.clone(), MailAction::Trash).await;
    let resolved_in = started.elapsed();
    assert_eq!(usize::try_from(counted.messages).unwrap(), BULK);
    assert_eq!(counted.message_ids, Vec::<MessageId>::new());

    let mut events = f.service.subscribe();
    let started = std::time::Instant::now();
    let moved = apply(&f, list, MailAction::Trash).await;
    let applied_in = started.elapsed();
    assert_eq!(moved.message_ids.len(), BULK);
    assert_eq!(moved.op_ids.len(), BULK);
    assert_eq!(folder_total(&f, &bulk), 0);
    assert_eq!(folder_total(&f, &trash.id), u32::try_from(BULK).unwrap());
    let (kinds, lagged) = count_events(&mut events);
    assert!(!lagged, "{kinds:?}");
    let parts = BULK.div_ceil(nail_api::mail_actions::MAIL_ACTION_PART_MESSAGES);
    assert_eq!(
        kinds.get("MailChanged"),
        Some(&parts),
        "one per part: {kinds:?}"
    );
    assert!(
        kinds.values().sum::<usize>() < nail_core::EVENT_CAPACITY / 8,
        "far below the channel's capacity: {kinds:?}"
    );
    // One batch: one undo brings every message back.
    let batch = moved.batch_id.unwrap();
    let record = f
        .service
        .store()
        .mail_batch(nail_store::parse_mail_batch_id(batch.as_str()).unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(record.message_ids.len(), BULK);
    let started = std::time::Instant::now();
    undo(&f, batch).await;
    let undone_in = started.elapsed();
    assert_eq!(folder_total(&f, &bulk), u32::try_from(BULK).unwrap());
    assert_eq!(folder_total(&f, &trash.id), 0);
    let (kinds, lagged) = count_events(&mut events);
    assert!(!lagged, "{kinds:?}");
    assert!(
        kinds.values().sum::<usize>() < nail_core::EVENT_CAPACITY / 8,
        "{kinds:?}"
    );
    eprintln!(
        "{BULK} messages: resolved in {resolved_in:?}, moved in {applied_in:?}, undone in {undone_in:?}"
    );
}
