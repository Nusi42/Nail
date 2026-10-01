//! The Graph engine against the loopback mock Graph server: mail
//! (hierarchy, per-folder deltas, MIME, writes, Focused Inbox, sending,
//! shared mailboxes), throttling and batching, sign-in, settings
//! (categories, automatic replies, rules), directory (rooms, mailboxes,
//! free/busy, meeting times) and PIM (calendars, contacts, To Do).

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use nail_api::ErrorCode;
use nail_api::accounts::{AuthConfig, IncomingServer, SecretRef, ServerEndpoint, TlsMode};
use nail_api::domain::Flags;
use nail_api::engine::{
    AccessTokenProvider, BoxFuture, DeltaOutcome, EngineError, Envelope, FreeBusyProvider,
    OutgoingTransport, RemoteWrite, SyncDelta, WriteOutcome,
};
use nail_api::exchange::engine::{
    ExchangeDirectoryEngine, ExchangeEngine, ExchangeMailEngine, ExchangePimEngine,
    ExchangePimWrite, ExchangePimWriteOutcome, ExchangeSettingsEngine, HierarchyDelta,
    HierarchyOutcome, MeetingSend, PimDelta, PimDeltaOutcome,
};
use nail_api::exchange::oof::{AutomaticRepliesStatus, ExternalAudience};
use nail_api::exchange::pim::{
    ChecklistItem, EventKind, ExchangeDateTime, ExchangeEvent, ExchangePimFolder, ExchangePimItem,
    ExchangeTask, PimFolderKind,
};
use nail_api::exchange::rules::{ExchangeInboxRule, ExchangeRuleActions, ExchangeRuleConditions};
use nail_api::exchange::{
    AutomaticRepliesSettings, CategoryColor, ExchangeCategorySettings, ExchangeCollectionRole,
    ExchangeEngine as EngineKind, ExchangeOptions, ExchangeWellKnownFolder,
    InferenceClassification,
};
use nail_api::ops::FlagChange;
use nail_api::scheduling::{BusyKind, FreeBusyQuery, InvitationResponse};
use nail_proto_graph::directory::MeetingTimesQuery;
use nail_proto_graph::pim::{DEFAULT_CONTACTS, split_todo_task_id};
use nail_proto_graph::testing::{InjectedError, MockGraphServer, fixtures};
use nail_proto_graph::wire::{Message, Page};
use nail_proto_graph::{
    EngineOptions, GraphAuth, GraphClient, GraphEngine, GraphError, RetryPolicy,
};
use pretty_assertions::assert_eq;
use serde_json::json;

const SHARED: &str = "support@contoso.example";

fn engine_for(server: &MockGraphServer, auth: GraphAuth) -> GraphEngine {
    let client =
        GraphClient::with_http(nail_auth::http_client().unwrap(), &server.api_base(), auth)
            .unwrap()
            .with_retry(RetryPolicy::fast());
    GraphEngine::new(client).with_account_id("work".into())
}

async fn demo() -> (MockGraphServer, GraphEngine) {
    let server = MockGraphServer::start_demo().await.unwrap();
    let engine = engine_for(&server, GraphAuth::Bearer("token".into()));
    (server, engine)
}

fn delta(outcome: DeltaOutcome) -> SyncDelta {
    match outcome {
        DeltaOutcome::Delta(d) => d,
        DeltaOutcome::CursorExpired => panic!("unexpected CursorExpired"),
    }
}

fn hierarchy(outcome: HierarchyOutcome) -> HierarchyDelta {
    match outcome {
        HierarchyOutcome::Delta(d) => d,
        HierarchyOutcome::CursorExpired => panic!("unexpected CursorExpired"),
    }
}

fn pim(outcome: PimDeltaOutcome) -> PimDelta {
    match outcome {
        PimDeltaOutcome::Delta(d) => d,
        PimDeltaOutcome::CursorExpired => panic!("unexpected CursorExpired"),
    }
}

/// Reads a folder's delta to the end; answers everything and the cursor.
async fn full_items(
    engine: &GraphEngine,
    mailbox: Option<&str>,
    folder: &str,
    cursor: Option<&str>,
) -> SyncDelta {
    let mut all = SyncDelta::default();
    let mut cursor = cursor.map(str::to_owned);
    for _ in 0..50 {
        let d = delta(
            engine
                .folder_items(mailbox, folder, cursor.as_deref(), None)
                .await
                .unwrap(),
        );
        all.added.extend(d.added);
        all.changed.extend(d.changed);
        all.removed.extend(d.removed);
        cursor = Some(d.cursor.clone());
        if !d.more {
            all.cursor = d.cursor;
            return all;
        }
    }
    panic!("delta did not end");
}

fn raw(from: &str, to: &str, subject: &str, id: &str) -> Vec<u8> {
    format!(
        "From: {from}\r\nTo: {to}\r\nSubject: {subject}\r\nDate: Thu, 01 Oct 2026 10:00:00 +0000\r\nMessage-ID: <{id}>\r\n\r\nHello.\r\n"
    )
    .into_bytes()
}

#[tokio::test]
async fn server_info_and_trait_objects() {
    let (_server, engine) = demo().await;
    let info = engine.server_info().await.unwrap();
    assert_eq!(info.address, "ada@contoso.example");
    assert_eq!(info.display_name.as_deref(), Some("Ada Lovelace"));
    assert_eq!(info.time_zone.as_deref(), Some("W. Europe Standard Time"));
    assert!(info.focused_inbox);
    let engine: Arc<dyn ExchangeEngine> = Arc::new(engine);
    assert_eq!(engine.kind(), EngineKind::Graph);
    assert_eq!(
        engine
            .wait_for_changes(None, &["inbox".into()], 30)
            .await
            .unwrap(),
        None,
        "Graph has no push: the core polls"
    );
}

#[tokio::test]
async fn folder_hierarchy_deltas() {
    let (server, engine) = demo().await;
    let first = hierarchy(engine.folder_hierarchy(None, None).await.unwrap());
    assert!(!first.more);
    let by_name = |d: &HierarchyDelta, name: &str| {
        d.upserted
            .iter()
            .find(|f| f.name == name)
            .unwrap_or_else(|| panic!("no folder {name}"))
            .clone()
    };
    let inbox = by_name(&first, "Inbox");
    assert_eq!(inbox.well_known, Some(ExchangeWellKnownFolder::Inbox));
    assert_eq!(inbox.parent, None, "children of the root have no parent");
    assert_eq!(inbox.total, Some(2));
    assert_eq!(inbox.unread, Some(1));
    let receipts = by_name(&first, "Receipts");
    assert_eq!(receipts.parent.as_deref(), Some(inbox.remote_id.as_str()));
    let engine_folder = by_name(&first, "Engine");
    assert_eq!(
        engine_folder.parent,
        Some(by_name(&first, "Projects").remote_id)
    );
    assert_eq!(
        by_name(&first, "Archive").well_known,
        Some(ExchangeWellKnownFolder::Archive)
    );
    assert!(
        first
            .upserted
            .iter()
            .all(|f| f.name != "Top of Information Store")
    );
    assert!(first.removed.is_empty());

    // Server-side changes arrive incrementally.
    let added = server.add_folder(None, Some("inbox"), "Travel");
    server
        .with_mailbox(None, |mb, _| mb.remove_folder(&receipts.remote_id))
        .unwrap();
    let next = hierarchy(
        engine
            .folder_hierarchy(None, Some(&first.cursor))
            .await
            .unwrap(),
    );
    assert_eq!(next.upserted.len(), 1);
    assert_eq!(next.upserted[0].remote_id, added);
    assert_eq!(next.removed, [receipts.remote_id.as_str()]);
    let quiet = hierarchy(
        engine
            .folder_hierarchy(None, Some(&next.cursor))
            .await
            .unwrap(),
    );
    assert!(quiet.upserted.is_empty() && quiet.removed.is_empty());

    // Expired and foreign cursors mean a fresh start.
    server.expire_deltas(None);
    assert_eq!(
        engine
            .folder_hierarchy(None, Some(&next.cursor))
            .await
            .unwrap(),
        HierarchyOutcome::CursorExpired
    );
    assert_eq!(
        engine
            .folder_hierarchy(None, Some("not a link"))
            .await
            .unwrap(),
        HierarchyOutcome::CursorExpired
    );
    assert_eq!(
        engine
            .folder_hierarchy(
                None,
                Some("https://evil.example/v1.0/me/mailFolders/delta?$deltatoken=1")
            )
            .await
            .unwrap(),
        HierarchyOutcome::CursorExpired
    );

    // Small pages: `more` until the end.
    let paged = GraphEngine::new(engine.client().clone()).with_options(EngineOptions {
        page_size: 2,
        pages_per_call: 1,
        ..EngineOptions::default()
    });
    let mut cursor = None;
    let mut names = Vec::new();
    for _ in 0..20 {
        let d = hierarchy(
            paged
                .folder_hierarchy(None, cursor.as_deref())
                .await
                .unwrap(),
        );
        names.extend(d.upserted.into_iter().map(|f| f.name));
        cursor = Some(d.cursor);
        if !d.more {
            break;
        }
    }
    assert!(names.len() >= 9, "{names:?}");

    // A shared mailbox without an Archive folder.
    let shared = hierarchy(engine.folder_hierarchy(Some(SHARED), None).await.unwrap());
    assert!(shared.upserted.iter().any(|f| f.name == "Escalations"));
    assert!(
        shared
            .upserted
            .iter()
            .all(|f| f.well_known != Some(ExchangeWellKnownFolder::Archive))
    );
}

#[tokio::test]
async fn message_deltas_per_folder() {
    let (server, engine) = demo().await;
    let inbox_id = server.folder_id(None, "inbox");
    let first = full_items(&engine, None, &inbox_id, None).await;
    assert_eq!(first.added.len(), 2);
    let review = first
        .added
        .iter()
        .find(|m| m.flags.flagged)
        .expect("the flagged review mail");
    assert!(!review.flags.seen);
    assert_eq!(review.tags, ["Red category"]);
    assert_eq!(review.folder, inbox_id);
    assert!(review.thread_id.is_some());
    assert_eq!(review.internal_date, Some(1_790_752_364));
    let info = review.exchange.clone().unwrap();
    assert_eq!(info.inference, Some(InferenceClassification::Focused));
    assert_eq!(info.mailbox, None);
    assert!(review.change_key.is_some());
    let news = first.added.iter().find(|m| m.flags.seen).unwrap();
    assert_eq!(
        news.exchange.as_ref().unwrap().inference,
        Some(InferenceClassification::Other)
    );

    // No Focused Inbox outside the inbox.
    let sent = full_items(&engine, None, "sentitems", None).await;
    assert_eq!(sent.added.len(), 1);
    assert_eq!(sent.added[0].exchange.as_ref().unwrap().inference, None);

    // Changes, moves and deletions on the server.
    let review_id = review.remote_id.clone();
    server.change_message(None, &review_id, |m| m.is_read = true);
    let new_id = server.deliver(
        None,
        "inbox",
        &raw("x@fabrikam.example", "ada@contoso.example", "New", "n1@x"),
    );
    let next = full_items(&engine, None, &inbox_id, Some(&first.cursor)).await;
    assert_eq!(next.added.len(), 2);
    assert!(
        next.added
            .iter()
            .any(|m| m.remote_id == review_id && m.flags.seen)
    );
    assert!(next.added.iter().any(|m| m.remote_id == new_id));
    let archived = server.move_message(None, &new_id, "archive");
    let after_move = full_items(&engine, None, &inbox_id, Some(&next.cursor)).await;
    assert_eq!(after_move.removed, [new_id.as_str()]);
    let archive = full_items(&engine, None, "archive", None).await;
    assert_eq!(archive.added[0].remote_id, archived);
    let quiet = full_items(&engine, None, &inbox_id, Some(&after_move.cursor)).await;
    assert!(quiet.added.is_empty() && quiet.removed.is_empty());

    // `since` limits the first sync.
    let recent = engine
        .folder_items(None, &inbox_id, None, Some(1_790_755_000))
        .await
        .unwrap();
    assert_eq!(delta(recent).added.len(), 1, "only the newsletter is newer");

    // Paging.
    let paged = GraphEngine::new(engine.client().clone()).with_options(EngineOptions {
        page_size: 1,
        pages_per_call: 1,
        ..EngineOptions::default()
    });
    let page = delta(
        paged
            .folder_items(None, &inbox_id, None, None)
            .await
            .unwrap(),
    );
    assert!(page.more && page.added.len() == 1);
    let rest = full_items(&paged, None, &inbox_id, Some(&page.cursor)).await;
    assert_eq!(rest.added.len(), 1);

    // Expiry.
    server.expire_deltas(None);
    assert_eq!(
        engine
            .folder_items(None, &inbox_id, Some(&quiet.cursor), None)
            .await
            .unwrap(),
        DeltaOutcome::CursorExpired
    );
    let fresh = full_items(&engine, None, &inbox_id, None).await;
    server.change_message(None, &review_id, |m| m.is_read = false);
    server.inject(InjectedError::sync_state_not_found().on("deltatoken"));
    assert_eq!(
        engine
            .folder_items(None, &inbox_id, Some(&fresh.cursor), None)
            .await
            .unwrap(),
        DeltaOutcome::CursorExpired
    );
}

#[tokio::test]
async fn first_sync_error_is_not_a_cursor_expiry() {
    let (server, engine) = demo().await;
    server.inject(InjectedError::sync_state_not_found().on("messages/delta"));
    let err = engine.folder_items(None, "inbox", None, None).await;
    // Without a cursor, a 410 is an error, not "start again".
    assert!(err.is_err());
    let err = engine
        .folder_items(None, "AAMk-nope=", None, None)
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
}

#[tokio::test]
async fn recorded_delta_page_folds() {
    let page: Page<Message> = serde_json::from_str(fixtures::RECORDED_MESSAGES_DELTA).unwrap();
    assert!(page.next_link.is_some());
    assert_eq!(page.value.len(), 4);
    assert!(page.value[3].removed.is_some());
    assert_eq!(page.value[2].received_date_time, None, "a partial change");
}

#[tokio::test]
async fn mime_and_attachments() {
    let (server, engine) = demo().await;
    let receipts = server.with_mailbox(None, |mb, _| {
        mb.folders
            .values()
            .find(|f| f.name == "Receipts")
            .unwrap()
            .id
            .clone()
    });
    let message = server.messages_in(None, &receipts)[0].clone();
    let mime = engine.fetch_mime(None, &message.id).await.unwrap();
    assert_eq!(mime, message.raw);
    let attachments = engine.attachments(None, &message.id).await.unwrap();
    assert_eq!(attachments.len(), 1);
    assert_eq!(attachments[0].name.as_deref(), Some("receipt.pdf"));
    let pdf = engine
        .attachment_content(None, &message.id, &attachments[0].id)
        .await
        .unwrap();
    assert!(pdf.starts_with(b"%PDF-1.4"));
    let err = engine.fetch_mime(None, "AAMk-gone=").await.unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
}

#[tokio::test]
async fn replayed_writes() {
    let (server, engine) = demo().await;
    let inbox = server.messages_in(None, "inbox");
    let review = inbox
        .iter()
        .find(|m| m.flag == "flagged")
        .unwrap()
        .id
        .clone();

    let change = FlagChange {
        seen: Some(true),
        flagged: Some(false),
        answered: Some(true),
        add_tags: vec!["Partner".into(), "partner".into()],
        remove_tags: vec!["RED CATEGORY".into()],
        ..FlagChange::default()
    };
    let write = RemoteWrite::SetFlags {
        remote_id: review.clone(),
        change,
    };
    assert_eq!(
        engine.write(None, &write).await.unwrap(),
        WriteOutcome::Done { remote_id: None }
    );
    let stored = server.message(None, &review).unwrap();
    assert!(stored.is_read);
    assert_eq!(stored.flag, "notFlagged");
    assert_eq!(stored.categories, ["Partner"]);
    assert_eq!(stored.last_verb.as_deref(), Some("102"));

    // A change that changes nothing sends no PATCH.
    server.clear_requests();
    let same = RemoteWrite::SetFlags {
        remote_id: review.clone(),
        change: FlagChange {
            add_tags: vec!["partner".into()],
            ..FlagChange::default()
        },
    };
    engine.write(None, &same).await.unwrap();
    assert!(server.requests().iter().all(|r| r.method != "PATCH"));

    // Moves answer the id (immutable ids: the same); trash goes to
    // Deleted Items.
    let moved = engine
        .write(
            None,
            &RemoteWrite::Move {
                remote_id: review.clone(),
                from_folder: server.folder_id(None, "inbox"),
                to_folder: server.folder_id(None, "archive"),
            },
        )
        .await
        .unwrap();
    let WriteOutcome::Done {
        remote_id: Some(moved_id),
    } = moved
    else {
        panic!("a move answers the new id");
    };
    assert_eq!(moved_id, review, "immutable ids survive a move");
    assert!(
        server.requests().iter().filter(|r| !r.in_batch).all(|r| r
            .prefer
            .as_deref()
            .is_some_and(|p| p.contains("IdType=\"ImmutableId\""))),
        "every request asks for immutable ids"
    );
    assert_eq!(
        server.message(None, &moved_id).unwrap().folder,
        server.folder_id(None, "archive")
    );
    let WriteOutcome::Done {
        remote_id: Some(trashed),
    } = engine
        .write(
            None,
            &RemoteWrite::Trash {
                remote_id: moved_id.clone(),
            },
        )
        .await
        .unwrap()
    else {
        panic!("trash answers the new id");
    };
    assert_eq!(
        server.message(None, &trashed).unwrap().folder,
        server.folder_id(None, "deleteditems")
    );
    assert_eq!(trashed, moved_id);
    assert_eq!(
        engine
            .write(
                None,
                &RemoteWrite::Delete {
                    remote_id: trashed.clone()
                }
            )
            .await
            .unwrap(),
        WriteOutcome::Done { remote_id: None }
    );
    assert!(server.message(None, &trashed).is_none());
    assert_eq!(
        engine
            .write(None, &RemoteWrite::Delete { remote_id: trashed })
            .await
            .unwrap(),
        WriteOutcome::Gone
    );
    assert_eq!(
        engine
            .write(
                None,
                &RemoteWrite::SetFlags {
                    remote_id: "AAMk-gone=".into(),
                    change: FlagChange {
                        seen: Some(true),
                        ..FlagChange::default()
                    }
                }
            )
            .await
            .unwrap(),
        WriteOutcome::Gone
    );

    // Copies.
    let news = server.messages_in(None, "inbox")[0].id.clone();
    let copy = engine
        .copy_message(None, &news, &server.folder_id(None, "archive"))
        .await
        .unwrap();
    assert!(server.message(None, &copy).is_some() && server.message(None, &news).is_some());
}

#[tokio::test]
async fn appends_are_idempotent() {
    let (server, engine) = demo().await;
    let drafts = server.folder_id(None, "drafts");
    let message = raw(
        "ada@contoso.example",
        "bob@contoso.example",
        "Draft",
        "draft.9@contoso.example",
    );
    let append = RemoteWrite::Append {
        folder: drafts.clone(),
        raw: message.clone(),
        flags: Flags {
            seen: true,
            flagged: true,
            ..Flags::default()
        },
    };
    let WriteOutcome::Done {
        remote_id: Some(id),
    } = engine.write(None, &append).await.unwrap()
    else {
        panic!("append answers the id");
    };
    let stored = server.message(None, &id).unwrap();
    assert_eq!(stored.raw, message);
    assert!(stored.is_read && stored.flag == "flagged" && stored.is_draft);
    let posts = server
        .requests()
        .into_iter()
        .filter(|r| r.method == "POST" && r.path.ends_with("/messages"))
        .collect::<Vec<_>>();
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].content_type.as_deref(), Some("text/plain"));
    // Replaying finds the copy.
    let again = engine.write(None, &append).await.unwrap();
    assert_eq!(
        again,
        WriteOutcome::Done {
            remote_id: Some(id)
        }
    );
    assert_eq!(server.messages_in(None, "drafts").len(), 1);
}

#[tokio::test]
async fn focused_inbox_classification() {
    let (server, engine) = demo().await;
    let ids: Vec<String> = server
        .messages_in(None, "inbox")
        .into_iter()
        .map(|m| m.id)
        .collect();
    engine
        .classify(None, &ids, InferenceClassification::Other, false)
        .await
        .unwrap();
    assert!(
        server
            .messages_in(None, "inbox")
            .iter()
            .all(|m| m.inference == "other")
    );
    assert!(server.with_mailbox(None, |mb, _| mb.overrides.is_empty()));
    engine
        .classify(None, &ids, InferenceClassification::Focused, true)
        .await
        .unwrap();
    assert!(
        server
            .messages_in(None, "inbox")
            .iter()
            .all(|m| m.inference == "focused")
    );
    let overrides = server.with_mailbox(None, |mb, _| mb.overrides.clone());
    assert_eq!(overrides.len(), 2, "one per sender");
    // Again: overrides are updated, not duplicated.
    engine
        .classify(None, &ids[..1], InferenceClassification::Other, true)
        .await
        .unwrap();
    let overrides = server.with_mailbox(None, |mb, _| mb.overrides.clone());
    assert_eq!(overrides.len(), 2);
    assert!(overrides.iter().any(|o| o["classifyAs"] == "other"));
    // Batched.
    assert!(
        server
            .requests()
            .iter()
            .any(|r| r.in_batch && r.method == "PATCH")
    );
    engine
        .classify(None, &[], InferenceClassification::Other, true)
        .await
        .unwrap();
}

#[tokio::test]
async fn sending_mime() {
    let (server, engine) = demo().await;
    let message = raw(
        "Ada <ada@contoso.example>",
        "bob@contoso.example",
        "Hi",
        "s1@contoso.example",
    );
    let envelope = Envelope {
        from: "ada@contoso.example".into(),
        to: vec!["bob@contoso.example".into(), "carol@contoso.example".into()],
    };
    let receipt = engine.send(&envelope, &message).await.unwrap();
    assert!(!receipt.response.is_empty());
    let sent = server.sent();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].raw.starts_with(b"Bcc: carol@contoso.example\r\n"));
    let copy = server
        .messages_in(None, "sentitems")
        .into_iter()
        .find(|m| {
            m.raw.ends_with(b"Hello.\r\n")
                && String::from_utf8_lossy(&m.raw).contains("Subject: Hi")
        })
        .unwrap();
    assert!(
        !String::from_utf8_lossy(&copy.raw).contains("carol"),
        "no Bcc in the copy"
    );

    // As a shared mailbox.
    engine
        .send_mime(
            Some(SHARED),
            &envelope,
            &raw(SHARED, "bob@contoso.example", "Ticket", "s2@x"),
        )
        .await
        .unwrap();
    assert_eq!(server.sent()[1].mailbox, SHARED);
    // A mailbox the user may not send as.
    let err = engine
        .send_mime(Some("ceo@contoso.example"), &envelope, &message)
        .await
        .unwrap_err();
    assert!(matches!(err, GraphError::Forbidden(_)));

    // No recipients, header injection, size.
    let none = Envelope {
        from: "ada@contoso.example".into(),
        to: vec![],
    };
    let err = engine
        .send(&none, b"Subject: x\r\n\r\nx")
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Rejected);
    let bad = Envelope {
        from: "ada@contoso.example".into(),
        to: vec!["x@y.example\r\nBcc: evil@z.example".into()],
    };
    assert_eq!(
        engine.send(&bad, &message).await.unwrap_err().code,
        ErrorCode::Unsupported
    );
    let huge = vec![b'a'; 4 * 1024 * 1024];
    assert_eq!(
        engine.send(&envelope, &huge).await.unwrap_err().code,
        ErrorCode::TooLarge
    );
    assert_eq!(server.sent().len(), 2);
}

#[tokio::test]
async fn throttling_and_server_errors() {
    let (server, engine) = demo().await;
    // A short Retry-After is waited for inline.
    server.inject(InjectedError::throttled(Some(0)).on("/me").times(2));
    assert!(engine.client().me(None).await.is_ok());
    // A long one goes up with the wait.
    server.inject(InjectedError::throttled(Some(120)).on("/me"));
    let err: EngineError = engine.client().me(None).await.unwrap_err().into();
    assert_eq!(err.code, ErrorCode::RateLimited);
    assert_eq!(err.retry_after_secs, Some(120));
    assert!(err.retryable);
    // 503 on a read is retried.
    server.inject(InjectedError::unavailable(None).on("/me"));
    assert!(engine.client().me(None).await.is_ok());
    // A send is not repeated after a 5xx (it may have gone out).
    server.inject(InjectedError::server_error().on("sendMail"));
    let envelope = Envelope {
        from: "ada@contoso.example".into(),
        to: vec!["bob@contoso.example".into()],
    };
    let message = raw("ada@contoso.example", "bob@contoso.example", "x", "t1@x");
    let err = engine.send(&envelope, &message).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::Unavailable);
    assert!(server.sent().is_empty());
    // But a throttled send is: it was refused before processing.
    server.inject(InjectedError::throttled(Some(0)).on("sendMail"));
    engine.send(&envelope, &message).await.unwrap();
    assert_eq!(server.sent().len(), 1);
    // A 503 send is not repeated either.
    server.inject(InjectedError::unavailable(Some(0)).on("sendMail"));
    let err = engine.send(&envelope, &message).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::RateLimited);
    assert_eq!(server.sent().len(), 1);
    // The request budget.
    server.set_request_budget(Some(0), 600);
    let err: EngineError = engine.client().me(None).await.unwrap_err().into();
    assert_eq!(err.retry_after_secs, Some(600));
    server.set_request_budget(None, 0);
    // Errors carry texts.
    server.inject(InjectedError::access_denied().on("/me"));
    let err: EngineError = engine.client().me(None).await.unwrap_err().into();
    assert_eq!(err.code, ErrorCode::Forbidden);
    assert_eq!(err.localized.unwrap().id, "graph-forbidden");
}

#[derive(Debug, Default)]
struct Tokens {
    calls: AtomicU32,
    forced: AtomicU32,
    revoked: bool,
}

impl AccessTokenProvider for Tokens {
    fn access_token<'a>(
        &'a self,
        provider: &'a str,
        _secret: &'a SecretRef,
        force_refresh: bool,
    ) -> BoxFuture<'a, Result<String, EngineError>> {
        Box::pin(async move {
            assert_eq!(provider, "microsoft");
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.revoked {
                return Err(EngineError::permanent(ErrorCode::ReauthRequired, "revoked"));
            }
            if force_refresh {
                self.forced.fetch_add(1, Ordering::SeqCst);
                Ok("fresh".to_owned())
            } else {
                Ok("stale".to_owned())
            }
        })
    }
}

fn oauth(tokens: Arc<Tokens>) -> GraphAuth {
    GraphAuth::OAuth2 {
        tokens,
        provider: "microsoft".into(),
        secret: SecretRef::parse("keychain:nail/work/oauth2").unwrap(),
    }
}

#[tokio::test]
async fn bearer_tokens_refresh_once() {
    let server = MockGraphServer::start_demo().await.unwrap();
    server.set_token(Some("fresh"));
    let tokens = Arc::new(Tokens::default());
    let engine = engine_for(&server, oauth(tokens.clone()));
    engine.server_info().await.unwrap();
    assert_eq!(tokens.forced.load(Ordering::SeqCst), 1);
    assert!(
        server
            .requests()
            .iter()
            .any(|r| r.token.as_deref() == Some("fresh"))
    );

    server.set_token(Some("other"));
    let err = engine.server_info().await.unwrap_err();
    assert_eq!(err.code, ErrorCode::AuthFailed);
    assert_eq!(err.localized.unwrap().id, "graph-auth-failed");

    let revoked = Arc::new(Tokens {
        revoked: true,
        ..Tokens::default()
    });
    let engine = engine_for(&server, oauth(revoked));
    let err = engine.server_info().await.unwrap_err();
    assert_eq!(
        err.code,
        ErrorCode::ReauthRequired,
        "the provider's error passes through"
    );
}

#[tokio::test]
async fn batches_split_and_retry_throttled_parts() {
    use nail_proto_graph::batch::BatchRequest;
    let (server, engine) = demo().await;
    let requests: Vec<BatchRequest> = (0..45)
        .map(|i| {
            BatchRequest::get(if i % 2 == 0 {
                "/me"
            } else {
                "/me/mailFolders/inbox"
            })
        })
        .collect();
    server.inject(
        InjectedError::throttled(Some(0))
            .on("/v1.0/me/mailFolders/inbox")
            .method("GET")
            .times(3),
    );
    server.clear_requests();
    let answers = engine.client().batch(&requests).await.unwrap();
    assert_eq!(answers.len(), 45);
    assert!(
        answers.iter().all(|a| a.ok()),
        "throttled parts were retried"
    );
    let batches = server
        .requests()
        .iter()
        .filter(|r| !r.in_batch && r.path.ends_with("$batch"))
        .count();
    assert_eq!(batches, 4, "3 batches of at most 20, then one retry round");
    // Long waits are left to the caller.
    server.inject(
        InjectedError::throttled(Some(300))
            .on("/v1.0/me/mailFolders/inbox")
            .method("GET"),
    );
    let answers = engine.client().batch(&requests[..2]).await.unwrap();
    assert_eq!(
        answers[1].error("x"),
        Some(GraphError::Throttled(Some(300)))
    );
    assert!(answers[0].ok());
    assert!(engine.client().batch(&[]).await.unwrap().is_empty());
    // The recorded answer parses.
    let recorded: serde_json::Value = serde_json::from_str(fixtures::RECORDED_BATCH).unwrap();
    assert_eq!(recorded["responses"].as_array().unwrap().len(), 4);
}

#[tokio::test]
async fn shared_and_foreign_mailboxes() {
    let (_server, engine) = demo().await;
    assert_eq!(
        engine.check_mailbox(SHARED).await.unwrap().as_deref(),
        Some("Contoso Support")
    );
    let denied = engine
        .check_mailbox("ceo@contoso.example")
        .await
        .unwrap_err();
    assert_eq!(denied.code, ErrorCode::Forbidden);
    let missing = engine
        .check_mailbox("nobody@contoso.example")
        .await
        .unwrap_err();
    assert_eq!(missing.code, ErrorCode::NotFound);
    let on_premises = engine
        .check_mailbox("legacy@contoso.example")
        .await
        .unwrap_err();
    assert_eq!(on_premises.code, ErrorCode::NotFound);
    assert_eq!(
        on_premises.localized.unwrap().id,
        "graph-mailbox-unavailable"
    );

    let items = full_items(&engine, Some(SHARED), "inbox", None).await;
    assert_eq!(items.added.len(), 1);
    assert_eq!(
        items.added[0].exchange.as_ref().unwrap().mailbox.as_deref(),
        Some(SHARED)
    );
    let mime = engine
        .fetch_mime(Some(SHARED), &items.added[0].remote_id)
        .await
        .unwrap();
    assert!(String::from_utf8_lossy(&mime).contains("Ticket 4711"));
    let err = engine
        .folder_hierarchy(Some("ceo@contoso.example"), None)
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Forbidden);
}

#[tokio::test]
async fn folders_are_created_renamed_and_deleted() {
    let (server, engine) = demo().await;
    let id = engine.create_folder(None, None, "Clients").await.unwrap();
    let child = engine.create_folder(None, Some(&id), "Acme").await.unwrap();
    engine
        .rename_folder(None, &child, "Acme Corp")
        .await
        .unwrap();
    assert_eq!(
        server.with_mailbox(None, |mb, _| mb.folders[&child].name.clone()),
        "Acme Corp"
    );
    let err = engine
        .create_folder(None, None, "clients")
        .await
        .unwrap_err();
    assert!(matches!(err, GraphError::Conflict(_)));
    engine.delete_folder(None, &id).await.unwrap();
    engine.delete_folder(None, &id).await.unwrap();
    assert!(server.with_mailbox(None, |mb, _| !mb.folders.contains_key(&child)));
}

#[tokio::test]
async fn master_categories() {
    let (server, engine) = demo().await;
    let categories = engine.categories().await.unwrap();
    assert_eq!(categories.len(), 3);
    let red = categories
        .iter()
        .find(|c| c.name == "Red category")
        .unwrap();
    assert_eq!(red.color, CategoryColor::Preset0);
    let id = engine
        .create_category(&ExchangeCategorySettings {
            name: "Urgent".into(),
            color: CategoryColor::Preset15,
        })
        .await
        .unwrap();
    // Replaying a create finds the category.
    let again = engine
        .create_category(&ExchangeCategorySettings::named("URGENT"))
        .await
        .unwrap();
    assert_eq!(again, id);
    // Colour changes are patched.
    engine
        .update_category(
            &id,
            &ExchangeCategorySettings {
                name: "Urgent".into(),
                color: CategoryColor::Preset1,
            },
        )
        .await
        .unwrap();
    assert!(
        engine
            .categories()
            .await
            .unwrap()
            .iter()
            .any(|c| c.id == id && c.color == CategoryColor::Preset1)
    );
    // Renames replace the category (Graph cannot rename).
    let new_id = engine
        .change_category(&id, &ExchangeCategorySettings::named("Very urgent"))
        .await
        .unwrap()
        .unwrap();
    assert_ne!(new_id, id);
    let names: Vec<String> = engine
        .categories()
        .await
        .unwrap()
        .into_iter()
        .map(|c| c.name)
        .collect();
    assert!(names.contains(&"Very urgent".to_owned()) && !names.contains(&"Urgent".to_owned()));
    // A replayed rename finds the result.
    assert_eq!(
        engine
            .change_category(&id, &ExchangeCategorySettings::named("Very urgent"))
            .await
            .unwrap(),
        Some(new_id.clone())
    );
    assert!(
        engine
            .change_category("missing", &ExchangeCategorySettings::named("Nope"))
            .await
            .is_err()
    );
    engine.delete_category(&new_id).await.unwrap();
    engine.delete_category(&new_id).await.unwrap();
    assert_eq!(engine.categories().await.unwrap().len(), 3);
    // Graph refuses colours outside its presets and renames by PATCH.
    let http = nail_auth::http_client().unwrap();
    let red_url = format!(
        "{}/me/outlook/masterCategories/{}",
        server.api_base(),
        red.id
    );
    for body in [
        json!({"color": "purple"}),
        json!({"displayName": "Crimson"}),
    ] {
        let status = http
            .patch(&red_url)
            .bearer_auth("t")
            .json(&body)
            .send()
            .await
            .unwrap()
            .status();
        assert_eq!(status.as_u16(), 400, "{body}");
    }
}

#[tokio::test]
async fn automatic_replies() {
    let (server, engine) = demo().await;
    let current = engine.automatic_replies().await.unwrap();
    assert_eq!(current.status, AutomaticRepliesStatus::Scheduled);
    assert_eq!(current.start.as_deref(), Some("2026-10-19T07:00:00Z"));
    assert_eq!(current.external_audience, ExternalAudience::ContactsOnly);
    assert_eq!(
        current.internal_reply,
        "I am at the conference until Friday.\nBob covers for me."
    );
    let settings = AutomaticRepliesSettings {
        status: AutomaticRepliesStatus::AlwaysEnabled,
        start: None,
        end: None,
        internal_reply: "Out <today>.\nBack tomorrow.".into(),
        external_reply: "Out today.".into(),
        external_audience: ExternalAudience::All,
    };
    engine.set_automatic_replies(&settings).await.unwrap();
    let stored = server.with_mailbox(None, |mb, _| mb.automatic_replies.clone());
    assert!(
        stored["internalReplyMessage"]
            .as_str()
            .unwrap()
            .contains("Out &lt;today&gt;.<br>Back tomorrow.")
    );
    let back = engine.automatic_replies().await.unwrap();
    assert_eq!(back.status, settings.status);
    assert_eq!(back.internal_reply, settings.internal_reply);
    assert_eq!(back.external_reply, settings.external_reply);
    // A shared mailbox's.
    assert_eq!(
        engine
            .read_automatic_replies(Some(SHARED))
            .await
            .unwrap()
            .status,
        AutomaticRepliesStatus::Disabled
    );
}

#[tokio::test]
async fn inbox_rules() {
    let (server, engine) = demo().await;
    let rules = engine.inbox_rules().await.unwrap();
    assert_eq!(rules.len(), 2);
    assert_eq!(rules[0].name, "Newsletters");
    assert!(!rules[0].has_unsupported);
    assert!(rules[1].has_unsupported, "isAutomaticReply is not modelled");
    let archive = server.folder_id(None, "archive");
    let rule = ExchangeInboxRule {
        id: String::new(),
        name: "Nail: receipts".into(),
        sequence: 3,
        enabled: true,
        conditions: ExchangeRuleConditions {
            from_addresses: vec!["store@contoso.example".into()],
            ..ExchangeRuleConditions::default()
        },
        exceptions: ExchangeRuleConditions::default(),
        actions: ExchangeRuleActions {
            move_to_folder: Some(archive.as_str().into()),
            ..ExchangeRuleActions::default()
        },
        has_unsupported: false,
        in_error: false,
        nail_rule_id: None,
    };
    let id = engine.create_inbox_rule(&rule).await.unwrap();
    let created = engine
        .inbox_rules()
        .await
        .unwrap()
        .into_iter()
        .find(|r| r.id == id)
        .unwrap();
    assert_eq!(created.conditions.from_addresses, ["store@contoso.example"]);
    assert_eq!(
        created.actions.move_to_folder,
        Some(archive.as_str().into())
    );
    let mut changed = created.clone();
    changed.enabled = false;
    changed.actions.mark_as_read = true;
    engine.update_inbox_rule(&changed).await.unwrap();
    let after = engine
        .inbox_rules()
        .await
        .unwrap()
        .into_iter()
        .find(|r| r.id == id)
        .unwrap();
    assert!(!after.enabled && after.actions.mark_as_read);
    // An Outlook rule only switches on / off.
    let mut outlook = rules[1].clone();
    outlook.enabled = true;
    outlook.name = "renamed".into();
    server.clear_requests();
    engine.update_inbox_rule(&outlook).await.unwrap();
    let patch = server
        .requests()
        .into_iter()
        .find(|r| r.method == "PATCH")
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&patch.body).unwrap(),
        json!({"isEnabled": true})
    );
    engine.delete_inbox_rule(&id).await.unwrap();
    engine.delete_inbox_rule(&id).await.unwrap();
    assert_eq!(engine.inbox_rules().await.unwrap().len(), 2);
    // A rule moving to a missing folder is refused.
    let mut broken = rule.clone();
    broken.actions.move_to_folder = Some("AAMk-missing=".into());
    assert_eq!(
        engine.create_inbox_rule(&broken).await.unwrap_err().code,
        ErrorCode::Rejected
    );
}

#[tokio::test]
async fn rooms_and_room_lists() {
    let (server, engine) = demo().await;
    let lists = engine.room_lists().await.unwrap();
    assert_eq!(
        lists.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(),
        ["Building 1", "Building 2"]
    );
    let all = engine.rooms(None).await.unwrap();
    assert_eq!(all.len(), 3);
    let adams = &all[0];
    assert_eq!(adams.address, "adams@contoso.example");
    assert_eq!(adams.capacity, Some(12));
    assert_eq!(adams.floor.as_deref(), Some("2"));
    assert_eq!(adams.features, ["Surface Hub", "whiteboard"]);
    let b1 = engine
        .rooms(Some("building1@contoso.example"))
        .await
        .unwrap();
    assert_eq!(b1.len(), 2);
    assert!(
        b1.iter()
            .all(|r| r.room_list.as_deref() == Some("building1@contoso.example"))
    );
    // Paging of places.
    let small = engine.list_rooms(None).await.unwrap();
    assert_eq!(small.len(), 3);
    server.forbid_places(true);
    assert_eq!(
        engine.room_lists().await.unwrap_err().code,
        ErrorCode::Forbidden
    );
}

#[tokio::test]
async fn free_busy_and_meeting_times() {
    let (_server, engine) = demo().await;
    let query = FreeBusyQuery {
        organizer: "ada@contoso.example".into(),
        attendees: vec![
            "bob@contoso.example".into(),
            "adams@contoso.example".into(),
            "nobody@contoso.example".into(),
        ],
        from: "2026-10-05T00:00:00Z".into(),
        to: "2026-10-06T00:00:00Z".into(),
    };
    let answers = engine.free_busy(&query).await.unwrap();
    assert_eq!(answers.len(), 3);
    assert_eq!(answers[0].email, "bob@contoso.example");
    assert_eq!(answers[0].periods.len(), 2);
    assert_eq!(answers[0].periods[0].start, "2026-10-05T07:00:00Z");
    assert_eq!(answers[0].periods[1].kind, BusyKind::Tentative);
    assert_eq!(answers[1].periods[0].kind, BusyKind::Busy);
    assert_eq!(answers[2].error, Some(ErrorCode::NotFound));
    assert!(matches!(
        &answers[0].source,
        nail_api::scheduling::FreeBusySource::Exchange { account_id } if account_id.as_str() == "work"
    ));
    // More than one getSchedule call for many attendees.
    let many = FreeBusyQuery {
        attendees: (0..45).map(|i| format!("p{i}@contoso.example")).collect(),
        ..query.clone()
    };
    assert_eq!(engine.free_busy(&many).await.unwrap().len(), 45);

    let suggestions = engine
        .find_meeting_times(&MeetingTimesQuery {
            attendees: vec!["bob@contoso.example".into()],
            rooms: vec!["adams@contoso.example".into()],
            from: "2026-10-05T07:00:00Z".into(),
            to: "2026-10-05T11:00:00Z".into(),
            duration_minutes: 60,
            max_candidates: 5,
        })
        .await
        .unwrap();
    assert_eq!(
        suggestions
            .iter()
            .map(|s| s.start.as_str())
            .collect::<Vec<_>>(),
        ["2026-10-05T08:00:00Z", "2026-10-05T10:00:00Z"]
    );
    assert_eq!(suggestions[0].rooms, ["adams@contoso.example"]);
}

async fn folders(engine: &GraphEngine, mailbox: Option<&str>) -> Vec<ExchangePimFolder> {
    engine.pim_folders(mailbox).await.unwrap()
}

fn folder(
    folders: &[ExchangePimFolder],
    kind: PimFolderKind,
    role: Option<ExchangeCollectionRole>,
) -> ExchangePimFolder {
    folders
        .iter()
        .find(|f| f.kind == kind && f.role == role)
        .cloned()
        .unwrap_or_else(|| panic!("no {kind:?} {role:?}"))
}

#[tokio::test]
async fn pim_folders() {
    let (server, engine) = demo().await;
    let all = folders(&engine, None).await;
    let calendar = folder(
        &all,
        PimFolderKind::Calendar,
        Some(ExchangeCollectionRole::DefaultCalendar),
    );
    assert_eq!(calendar.name, "Calendar");
    let holidays = all.iter().find(|f| f.name == "Holidays").unwrap();
    assert!(holidays.read_only);
    assert_eq!(holidays.color, Some(CategoryColor::Preset4));
    let contacts = folder(
        &all,
        PimFolderKind::Contacts,
        Some(ExchangeCollectionRole::DefaultContacts),
    );
    assert_eq!(
        contacts.remote_id,
        server.with_mailbox(None, |mb, _| mb.default_contacts.clone())
    );
    assert!(
        all.iter()
            .any(|f| f.kind == PimFolderKind::Contacts && f.name == "Partners")
    );
    let tasks = folder(
        &all,
        PimFolderKind::Tasks,
        Some(ExchangeCollectionRole::DefaultTasks),
    );
    assert!(tasks.todo && !tasks.read_only);
    let flagged = folder(
        &all,
        PimFolderKind::Tasks,
        Some(ExchangeCollectionRole::FlaggedEmails),
    );
    assert!(flagged.read_only);
    assert!(
        all.iter()
            .any(|f| f.name == "Groceries" && f.role.is_none())
    );
    // Another mailbox: no To Do; a mailbox without contacts uses the alias.
    let shared = folders(&engine, Some(SHARED)).await;
    assert!(shared.iter().all(|f| f.kind != PimFolderKind::Tasks));
    let alias = folder(
        &shared,
        PimFolderKind::Contacts,
        Some(ExchangeCollectionRole::DefaultContacts),
    );
    assert_eq!(alias.remote_id, DEFAULT_CONTACTS);
    let listing = pim(engine
        .pim_changes(Some(SHARED), &alias, None)
        .await
        .unwrap());
    assert!(listing.upserted.is_empty() && !listing.more);
    assert!(
        engine
            .pim_changes(Some(SHARED), &tasks, None)
            .await
            .is_err()
    );
}

fn events(delta: &PimDelta) -> Vec<ExchangeEvent> {
    delta
        .upserted
        .iter()
        .filter_map(|i| match i {
            ExchangePimItem::Event(e) => Some((**e).clone()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn calendar_deltas() {
    let (server, engine) = demo().await;
    let all = folders(&engine, None).await;
    let calendar = folder(
        &all,
        PimFolderKind::Calendar,
        Some(ExchangeCollectionRole::DefaultCalendar),
    );
    let first = pim(engine.pim_changes(None, &calendar, None).await.unwrap());
    assert!(!first.more);
    let list = events(&first);
    assert_eq!(list.len(), 3);
    let series = list.iter().find(|e| e.subject == "Weekly sync").unwrap();
    assert_eq!(series.kind, EventKind::SeriesMaster);
    assert_eq!(series.start.time_zone, "W. Europe Standard Time");
    assert_eq!(series.start.date_time, "2026-10-05T09:00:00");
    assert!(series.recurrence.is_some());
    assert!(series.exceptions.is_empty());
    let invite = list
        .iter()
        .find(|e| e.subject == "Architecture review")
        .unwrap();
    assert!(!invite.is_organizer);
    // The events were read with their zone.
    assert!(server.requests().iter().any(|r| {
        r.in_batch
            && r.prefer
                .as_deref()
                .is_some_and(|p| p.contains("outlook.timezone=\"W. Europe Standard Time\""))
    }));

    // An exception and a cancelled occurrence on the server.
    let master = series.remote_id.clone();
    server.with_mailbox(None, |mb, _| {
        let mut exception = mb.events[&master].clone();
        exception.as_object_mut().unwrap().remove("recurrence");
        exception["id"] = json!(format!("{master}.occ.2026-10-19"));
        exception["type"] = json!("exception");
        exception["seriesMasterId"] = json!(master);
        exception["subject"] = json!("Weekly sync (moved)");
        exception["originalStart"] = json!("2026-10-19T09:00:00Z");
        exception["start"]["dateTime"] = json!("2026-10-19T11:00:00.0000000");
        exception["end"]["dateTime"] = json!("2026-10-19T11:30:00.0000000");
        let id = format!("{master}.occ.2026-10-19");
        mb.events.insert(id.clone(), exception);
        mb.events.get_mut(&master).unwrap()["cancelledOccurrences"] =
            json!([format!("OID.{master}.2026-10-12")]);
        mb.touch_event(&id);
    });
    let dentist = list
        .iter()
        .find(|e| e.subject == "Dentist")
        .unwrap()
        .remote_id
        .clone();
    server.delete_event(&dentist);
    let next = pim(engine
        .pim_changes(None, &calendar, Some(&first.cursor))
        .await
        .unwrap());
    assert_eq!(next.removed, [dentist]);
    let changed = events(&next);
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0].remote_id, master);
    assert_eq!(changed[0].cancelled_occurrences, ["2026-10-12"]);
    assert_eq!(changed[0].exceptions.len(), 1);
    assert_eq!(changed[0].exceptions[0].subject, "Weekly sync (moved)");
    assert_eq!(
        changed[0].exceptions[0].original_start,
        Some(ExchangeDateTime::utc("2026-10-19T09:00:00"))
    );
    // Instances of the series.
    let instances = engine
        .event_instances(
            None,
            &master,
            "2026-10-01T00:00:00Z",
            "2026-11-01T00:00:00Z",
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        instances.len(),
        3,
        "5, 19 (exception), 26; the 12th is cancelled"
    );

    server.expire_deltas(None);
    assert_eq!(
        engine
            .pim_changes(None, &calendar, Some(&next.cursor))
            .await
            .unwrap(),
        PimDeltaOutcome::CursorExpired
    );
    assert_eq!(
        engine
            .pim_changes(None, &calendar, Some("garbage"))
            .await
            .unwrap(),
        PimDeltaOutcome::CursorExpired
    );
}

fn single_event(subject: &str) -> ExchangeEvent {
    serde_json::from_value(json!({
        "remote_id": "", "subject": subject,
        "start": {"date_time": "2026-10-14T10:00:00", "time_zone": "W. Europe Standard Time"},
        "end": {"date_time": "2026-10-14T11:00:00", "time_zone": "W. Europe Standard Time"},
        "reminder_minutes": 10,
        "attendees": [{"email": {"address": "bob@contoso.example"}, "kind": "required"}],
        "locations": [{"display_name": "Conf Room Baker", "email": "baker@contoso.example", "room": true}]
    }))
    .unwrap()
}

#[tokio::test]
async fn event_writes() {
    let (server, engine) = demo().await;
    let all = folders(&engine, None).await;
    let calendar = folder(
        &all,
        PimFolderKind::Calendar,
        Some(ExchangeCollectionRole::DefaultCalendar),
    );
    let create = ExchangePimWrite::Create {
        folder: calendar.remote_id.clone(),
        item: ExchangePimItem::Event(Box::new(single_event("Planning"))),
        send: MeetingSend::ToAll,
    };
    let ExchangePimWriteOutcome::Done {
        remote_id,
        change_key,
    } = engine.write_pim(None, &create).await.unwrap()
    else {
        panic!("created");
    };
    let stored = server.event(&remote_id).unwrap();
    assert_eq!(stored["subject"], "Planning");
    assert_eq!(
        stored["location"]["locationEmailAddress"],
        "baker@contoso.example"
    );
    assert_eq!(stored["reminderMinutesBeforeStart"], 10);

    // Updates check the change key.
    let mut event = single_event("Planning (moved)");
    event.remote_id.clone_from(&remote_id);
    event.change_key.clone_from(&change_key);
    let update = ExchangePimWrite::Update {
        item: ExchangePimItem::Event(Box::new(event.clone())),
        send: MeetingSend::ToAll,
    };
    let ExchangePimWriteOutcome::Done {
        change_key: new_key,
        ..
    } = engine.write_pim(None, &update).await.unwrap()
    else {
        panic!("updated");
    };
    assert_ne!(new_key, change_key);
    assert_eq!(
        server.event(&remote_id).unwrap()["subject"],
        "Planning (moved)"
    );
    assert_eq!(
        engine.write_pim(None, &update).await.unwrap(),
        ExchangePimWriteOutcome::Conflict,
        "the old change key is stale"
    );

    // Deleting a meeting the user organizes cancels it.
    engine
        .write_pim(
            None,
            &ExchangePimWrite::Delete {
                remote_id: remote_id.clone(),
                send: MeetingSend::ToAll,
            },
        )
        .await
        .unwrap();
    assert!(server.event(&remote_id).is_none());
    assert_eq!(server.meeting_actions().last().unwrap().action, "cancel");
    assert_eq!(
        engine
            .write_pim(
                None,
                &ExchangePimWrite::Delete {
                    remote_id: remote_id.clone(),
                    send: MeetingSend::None
                }
            )
            .await
            .unwrap(),
        ExchangePimWriteOutcome::Gone
    );
    assert_eq!(
        engine.write_pim(None, &update).await.unwrap(),
        ExchangePimWriteOutcome::Gone
    );

    // A series with an exception and a cancelled occurrence.
    let mut series = single_event("Standup");
    series.recurrence = Some(
        serde_json::from_value(json!({
            "pattern": {"kind": "daily", "interval": 1},
            "range": {"kind": "numbered", "start_date": "2026-10-14", "occurrences": 5}
        }))
        .unwrap(),
    );
    let mut exception = single_event("Standup (late)");
    exception.kind = EventKind::Exception;
    exception.original_start = Some(ExchangeDateTime::utc("2026-10-15T10:00:00"));
    exception.start.date_time = "2026-10-15T12:00:00".into();
    exception.end.date_time = "2026-10-15T12:30:00".into();
    series.exceptions = vec![exception];
    series.cancelled_occurrences = vec!["2026-10-16".into()];
    let ExchangePimWriteOutcome::Done {
        remote_id: series_id,
        ..
    } = engine
        .write_pim(
            None,
            &ExchangePimWrite::Create {
                folder: calendar.remote_id.clone(),
                item: ExchangePimItem::Event(Box::new(series)),
                send: MeetingSend::None,
            },
        )
        .await
        .unwrap()
    else {
        panic!("created");
    };
    let master = server.event(&series_id).unwrap();
    assert_eq!(master["type"], "seriesMaster");
    assert_eq!(
        master["cancelledOccurrences"][0],
        format!("OID.{series_id}.2026-10-16")
    );
    let exception = server
        .event(&format!("{series_id}.occ.2026-10-15"))
        .unwrap();
    assert_eq!(exception["subject"], "Standup (late)");
    assert_eq!(exception["start"]["dateTime"], "2026-10-15T12:00:00");
}

#[tokio::test]
async fn meeting_responses() {
    let (server, engine) = demo().await;
    let (invite, _) = server
        .events()
        .into_iter()
        .find(|(_, e)| e["subject"] == "Architecture review")
        .unwrap();
    engine
        .respond_to_meeting(
            None,
            &invite,
            InvitationResponse::Accept,
            Some("See you"),
            true,
        )
        .await
        .unwrap();
    let action = server.meeting_actions().pop().unwrap();
    assert_eq!(action.action, "accept");
    assert_eq!(
        action.body,
        json!({"sendResponse": true, "comment": "See you"})
    );
    assert_eq!(
        server.event(&invite).unwrap()["responseStatus"]["response"],
        "accepted"
    );
    let start = ExchangeDateTime::utc("2026-10-08T15:00:00");
    let end = ExchangeDateTime::utc("2026-10-08T16:00:00");
    engine
        .propose_new_time(None, &invite, &start, &end, None, false)
        .await
        .unwrap();
    let action = server.meeting_actions().pop().unwrap();
    assert_eq!(action.action, "tentativelyAccept");
    assert_eq!(
        action.body["proposedNewTime"]["start"]["dateTime"],
        "2026-10-08T15:00:00"
    );
    engine
        .respond_to_meeting(None, &invite, InvitationResponse::Decline, None, false)
        .await
        .unwrap();
    assert_eq!(
        server.meeting_actions().pop().unwrap().body,
        json!({"sendResponse": false})
    );
    assert!(
        engine
            .respond(
                None,
                &invite,
                InvitationResponse::Accept,
                None,
                true,
                Some((&start, &end))
            )
            .await
            .is_err()
    );
    // Not for the user's own meetings, not for missing ones.
    let (own, _) = server
        .events()
        .into_iter()
        .find(|(_, e)| e["subject"] == "Dentist")
        .unwrap();
    assert_eq!(
        engine
            .respond_to_meeting(None, &own, InvitationResponse::Accept, None, true)
            .await
            .unwrap_err()
            .code,
        ErrorCode::Rejected
    );
    assert_eq!(
        engine
            .respond_to_meeting(None, "AAMk-gone=", InvitationResponse::Accept, None, true)
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
}

#[tokio::test]
async fn contacts() {
    let (server, engine) = demo().await;
    let all = folders(&engine, None).await;
    let default = folder(
        &all,
        PimFolderKind::Contacts,
        Some(ExchangeCollectionRole::DefaultContacts),
    );
    let partners = all.iter().find(|f| f.name == "Partners").unwrap().clone();
    let first = pim(engine.pim_changes(None, &default, None).await.unwrap());
    assert_eq!(first.upserted.len(), 1);
    let ExchangePimItem::Contact(bob) = &first.upserted[0] else {
        panic!("contact")
    };
    assert_eq!(bob.emails[0].address, "bob@contoso.example");
    let p = pim(engine.pim_changes(None, &partners, None).await.unwrap());
    assert_eq!(p.upserted.len(), 1);

    let mut new_contact = (**bob).clone();
    new_contact.display_name = "Carol Example".into();
    new_contact.emails[0].address = "carol@contoso.example".into();
    let ExchangePimWriteOutcome::Done {
        remote_id,
        change_key,
    } = engine
        .write_pim(
            None,
            &ExchangePimWrite::Create {
                folder: partners.remote_id.clone(),
                item: ExchangePimItem::Contact(Box::new(new_contact.clone())),
                send: MeetingSend::None,
            },
        )
        .await
        .unwrap()
    else {
        panic!("created");
    };
    assert_eq!(
        server.contact(&remote_id).unwrap()["displayName"],
        "Carol Example"
    );
    let changes = pim(engine
        .pim_changes(None, &partners, Some(&p.cursor))
        .await
        .unwrap());
    assert_eq!(changes.upserted.len(), 1);
    new_contact.remote_id.clone_from(&remote_id);
    new_contact.change_key = change_key;
    new_contact.job_title = Some("CTO".into());
    let update = ExchangePimWrite::Update {
        item: ExchangePimItem::Contact(Box::new(new_contact.clone())),
        send: MeetingSend::None,
    };
    assert!(matches!(
        engine.write_pim(None, &update).await.unwrap(),
        ExchangePimWriteOutcome::Done { .. }
    ));
    assert_eq!(server.contact(&remote_id).unwrap()["jobTitle"], "CTO");
    assert_eq!(
        engine.write_pim(None, &update).await.unwrap(),
        ExchangePimWriteOutcome::Conflict
    );
    engine
        .write_pim(
            None,
            &ExchangePimWrite::Delete {
                remote_id: remote_id.clone(),
                send: MeetingSend::None,
            },
        )
        .await
        .unwrap();
    assert!(server.contact(&remote_id).is_none());
    let gone = pim(engine
        .pim_changes(None, &partners, Some(&changes.cursor))
        .await
        .unwrap());
    assert_eq!(gone.removed, [remote_id.as_str()]);
    assert_eq!(
        engine
            .write_pim(
                None,
                &ExchangePimWrite::Delete {
                    remote_id,
                    send: MeetingSend::None
                }
            )
            .await
            .unwrap(),
        ExchangePimWriteOutcome::Gone
    );
    assert_eq!(engine.contact_photo(None, "AAMk-x=").await.unwrap(), None);

    // The alias of an empty mailbox's default folder takes creates.
    let empty = MockGraphServer::start("eve@contoso.example").await.unwrap();
    let eve = engine_for(&empty, GraphAuth::Bearer("t".into()));
    let eve_folders = folders(&eve, None).await;
    let alias = folder(
        &eve_folders,
        PimFolderKind::Contacts,
        Some(ExchangeCollectionRole::DefaultContacts),
    );
    assert_eq!(alias.remote_id, DEFAULT_CONTACTS);
    eve.write_pim(
        None,
        &ExchangePimWrite::Create {
            folder: DEFAULT_CONTACTS.into(),
            item: ExchangePimItem::Contact(Box::new(new_contact)),
            send: MeetingSend::None,
        },
    )
    .await
    .unwrap();
    let after = folders(&eve, None).await;
    let real = folder(
        &after,
        PimFolderKind::Contacts,
        Some(ExchangeCollectionRole::DefaultContacts),
    );
    assert_ne!(
        real.remote_id, DEFAULT_CONTACTS,
        "resolved from the contact's parent"
    );
    assert_eq!(
        pim(eve.pim_changes(None, &alias, None).await.unwrap())
            .upserted
            .len(),
        1
    );
}

#[tokio::test]
async fn todo_tasks() {
    let (server, engine) = demo().await;
    let all = folders(&engine, None).await;
    let tasks = folder(
        &all,
        PimFolderKind::Tasks,
        Some(ExchangeCollectionRole::DefaultTasks),
    );
    let first = pim(engine.pim_changes(None, &tasks, None).await.unwrap());
    let ExchangePimItem::Task(report) = &first.upserted[0] else {
        panic!("task")
    };
    assert_eq!(report.title, "Write the quarterly report");
    assert_eq!(
        split_todo_task_id(&report.remote_id).unwrap().0,
        tasks.remote_id
    );
    assert_eq!(report.checklist.len(), 2);
    assert!(report.checklist[0].checked);
    assert_eq!(
        report.linked_resources[0].external_id.as_deref(),
        Some("review.1")
    );

    let new_task = ExchangeTask {
        remote_id: String::new(),
        change_key: None,
        title: "Book travel".into(),
        checklist: vec![
            ChecklistItem {
                id: String::new(),
                title: "Flights".into(),
                checked: false,
            },
            ChecklistItem {
                id: String::new(),
                title: "Hotel".into(),
                checked: false,
            },
        ],
        ..(**report).clone()
    };
    let ExchangePimWriteOutcome::Done {
        remote_id,
        change_key,
    } = engine
        .write_pim(
            None,
            &ExchangePimWrite::Create {
                folder: tasks.remote_id.clone(),
                item: ExchangePimItem::Task(Box::new(new_task.clone())),
                send: MeetingSend::None,
            },
        )
        .await
        .unwrap()
    else {
        panic!("created");
    };
    let (_, task_id) = split_todo_task_id(&remote_id).unwrap();
    let stored = server.task(task_id).unwrap();
    assert_eq!(stored.value["title"], "Book travel");
    assert_eq!(stored.checklist.len(), 2);

    // The checklist follows the update: rename, remove, add.
    let changes = pim(engine
        .pim_changes(None, &tasks, Some(&first.cursor))
        .await
        .unwrap());
    let ExchangePimItem::Task(current) = changes
        .upserted
        .iter()
        .find(|i| i.remote_id() == remote_id)
        .unwrap()
    else {
        panic!("task");
    };
    let mut updated = (**current).clone();
    updated.checklist[0].checked = true;
    updated.checklist.remove(1);
    updated.checklist.push(ChecklistItem {
        id: String::new(),
        title: "Car".into(),
        checked: false,
    });
    updated.change_key.clone_from(&current.change_key);
    let update = ExchangePimWrite::Update {
        item: ExchangePimItem::Task(Box::new(updated)),
        send: MeetingSend::None,
    };
    assert!(matches!(
        engine.write_pim(None, &update).await.unwrap(),
        ExchangePimWriteOutcome::Done { .. }
    ));
    let stored = server.task(task_id).unwrap();
    let titles: Vec<(String, bool)> = stored
        .checklist
        .iter()
        .map(|c| {
            (
                c["displayName"].as_str().unwrap().to_owned(),
                c["isChecked"].as_bool().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        titles,
        [("Flights".to_owned(), true), ("Car".to_owned(), false)]
    );
    assert_ne!(change_key, None);
    assert_eq!(
        engine.write_pim(None, &update).await.unwrap(),
        ExchangePimWriteOutcome::Conflict
    );
    engine
        .write_pim(
            None,
            &ExchangePimWrite::Delete {
                remote_id: remote_id.clone(),
                send: MeetingSend::None,
            },
        )
        .await
        .unwrap();
    assert!(server.task(task_id).is_none());
    let gone = pim(engine
        .pim_changes(None, &tasks, Some(&changes.cursor))
        .await
        .unwrap());
    assert!(gone.removed.contains(&remote_id));
    // Not in other mailboxes.
    assert_eq!(
        engine
            .write_pim(
                Some(SHARED),
                &ExchangePimWrite::Create {
                    folder: tasks.remote_id.clone(),
                    item: ExchangePimItem::Task(Box::new(new_task)),
                    send: MeetingSend::None,
                }
            )
            .await
            .unwrap_err()
            .code,
        ErrorCode::Unsupported
    );
}

#[tokio::test]
async fn engine_for_configured_accounts() {
    let server = MockGraphServer::start_demo().await.unwrap();
    server.set_token(Some("fresh"));
    let incoming = IncomingServer::Exchange {
        server: ServerEndpoint {
            host: "127.0.0.1".into(),
            port: server.addr().port(),
            tls: TlsMode::Plaintext,
            auth: AuthConfig::OAuth2 {
                username: "ada@contoso.example".into(),
                provider: "microsoft".into(),
                secret: None,
                mechanism: None,
            },
        },
        options: ExchangeOptions::default(),
    };
    let mut account: nail_api::AccountConfig = serde_json::from_value(json!({
        "id": "work", "name": "Work", "email": "ada@contoso.example",
        "incoming": serde_json::to_value(&incoming).unwrap(),
    }))
    .unwrap();
    let engine = GraphEngine::for_account(&account, Arc::new(Tokens::default())).unwrap();
    assert_eq!(engine.account_id().as_str(), "work");
    assert_eq!(
        engine.server_info().await.unwrap().address,
        "ada@contoso.example"
    );

    if let IncomingServer::Exchange { options, .. } = &mut account.incoming {
        options.engine = EngineKind::Ews;
    }
    assert!(matches!(
        GraphEngine::for_account(&account, Arc::new(Tokens::default())),
        Err(GraphError::Config(_))
    ));
    if let IncomingServer::Exchange {
        options,
        server: endpoint,
    } = &mut account.incoming
    {
        options.engine = EngineKind::Graph;
        endpoint.auth = AuthConfig::None;
    }
    assert!(matches!(
        GraphEngine::for_account(&account, Arc::new(Tokens::default())),
        Err(GraphError::Config(_))
    ));
    if let IncomingServer::Exchange {
        server: endpoint, ..
    } = &mut account.incoming
    {
        endpoint.host = "graph.example".into();
        endpoint.auth = AuthConfig::OAuth2 {
            username: "a".into(),
            provider: "microsoft".into(),
            secret: None,
            mechanism: None,
        };
    }
    assert!(matches!(
        GraphEngine::for_account(&account, Arc::new(Tokens::default())),
        Err(GraphError::InsecureUrl(_))
    ));
}

#[tokio::test]
async fn mock_server_basics() {
    let server = MockGraphServer::start_demo().await.unwrap();
    assert!(server.addr().ip().is_loopback());
    assert!(MockGraphServer::start_fixture("{}").await.is_err());
    assert_eq!(server.me(), "ada@contoso.example");
    let http = nail_auth::http_client().unwrap();
    let get = |path: &str, token: Option<&str>| {
        let mut request = http.get(format!("{}{path}", server.api_base()));
        if let Some(t) = token {
            request = request.bearer_auth(t);
        }
        request.send()
    };
    assert_eq!(get("/me", None).await.unwrap().status().as_u16(), 401);
    assert_eq!(
        get("/nothing/here", Some("t"))
            .await
            .unwrap()
            .status()
            .as_u16(),
        400
    );
    assert_eq!(
        get("/me/calendarView/delta", Some("t"))
            .await
            .unwrap()
            .status()
            .as_u16(),
        400,
        "calendarView needs a window"
    );
    assert_eq!(
        get(
            "/me/mailFolders/inbox/messages/delta?$filter=subject%20eq%20'x'",
            Some("t")
        )
        .await
        .unwrap()
        .status()
        .as_u16(),
        400
    );
    let batch = http
        .post(format!("{}/$batch", server.api_base()))
        .bearer_auth("t")
        .json(&json!({"requests": (0..21).map(|i| json!({"id": i.to_string(), "method": "GET", "url": "/me"})).collect::<Vec<_>>()}))
        .send()
        .await
        .unwrap();
    assert_eq!(batch.status().as_u16(), 400, "at most 20 requests");
}
