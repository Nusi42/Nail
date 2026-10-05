//! The EWS engine against the mock EWS server: server info, versions,
//! folder hierarchy and items with sync states, MIME, replayed writes,
//! sending, logins, throttling and streaming notifications.
#![cfg(test)]

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use nail_api::ErrorCode;
use nail_api::accounts::SecretRef;
use nail_api::domain::Flags;
use nail_api::engine::{
    AccessTokenProvider, BoxFuture, DeltaOutcome, EngineError, Envelope, MemorySecretStore,
    OutgoingTransport, RemoteWrite, SecretStore, SyncDelta, WriteOutcome,
};
use nail_api::exchange::engine::{
    ExchangeFolderClass, ExchangeMailEngine, HierarchyDelta, HierarchyOutcome,
};
use nail_api::exchange::{ExchangeWellKnownFolder, InferenceClassification};
use nail_api::ops::FlagChange;
use nail_proto_ews::client::{CALL_TIMEOUT, lane_http};
use nail_proto_ews::testing::{
    FaultKind, InjectedFault, MockAuth, MockEwsServer, OWN_ADDRESS, SHARED_ADDRESS,
    ScriptedNegotiate, fixtures,
};
use nail_proto_ews::types::ItemRef;
use nail_proto_ews::{
    EwsAuth, EwsClient, EwsEngine, EwsError, EwsOptions, FALLBACK_SERVER_VERSION, Password,
    RetryPolicy,
};
use pretty_assertions::assert_eq;

fn client(server: &MockEwsServer, auth: EwsAuth) -> EwsClient {
    EwsClient::with_http(
        lane_http(CALL_TIMEOUT).unwrap(),
        lane_http(Duration::from_mins(2)).unwrap(),
        &server.url(),
        auth,
    )
    .unwrap()
    .with_own_address(OWN_ADDRESS)
    .with_retry(RetryPolicy::fast())
}

fn engine(server: &MockEwsServer) -> EwsEngine {
    EwsEngine::new(client(server, EwsAuth::None), "work".into())
}

fn delta(outcome: DeltaOutcome) -> SyncDelta {
    match outcome {
        DeltaOutcome::Delta(delta) => delta,
        DeltaOutcome::CursorExpired => panic!("unexpected CursorExpired"),
    }
}

fn hierarchy(outcome: HierarchyOutcome) -> HierarchyDelta {
    match outcome {
        HierarchyOutcome::Delta(delta) => delta,
        HierarchyOutcome::CursorExpired => panic!("unexpected CursorExpired"),
    }
}

#[tokio::test]
async fn server_info_and_versions() {
    let server = MockEwsServer::start().await.unwrap();
    let engine = engine(&server);
    let info = engine.server_info().await.unwrap();
    assert_eq!(info.address, OWN_ADDRESS);
    assert_eq!(info.display_name.as_deref(), Some("Ada Lovelace"));
    assert_eq!(
        info.server_version.as_deref(),
        Some("Exchange 2019 (15.2.1544.4)")
    );
    assert!(!info.focused_inbox);
    let log = server.requests();
    assert!(log.iter().all(|r| r.version == "Exchange2016"));
    assert!(log.iter().all(|r| r.anchor.as_deref() == Some(OWN_ADDRESS)));
    assert!(log.iter().all(|r| r.peer.ip().is_loopback()));

    // An older server refuses Exchange2016: the client falls back.
    let old = MockEwsServer::start().await.unwrap();
    old.refuse_versions(&["Exchange2016"]);
    old.set_server_version(15, 0, 1497, 2);
    let engine = EwsEngine::new(client(&old, EwsAuth::None), "work".into());
    let info = engine.server_info().await.unwrap();
    assert_eq!(
        info.server_version.as_deref(),
        Some("Exchange 2013 (15.0.1497.2)")
    );
    assert_eq!(engine.client().request_version(), FALLBACK_SERVER_VERSION);
    let versions: Vec<String> = old.requests().into_iter().map(|r| r.version).collect();
    assert_eq!(versions[0], "Exchange2016");
    assert!(versions[1..].iter().all(|v| v == FALLBACK_SERVER_VERSION));
}

#[tokio::test]
async fn folder_hierarchy_with_sync_states() {
    let server = MockEwsServer::start().await.unwrap();
    let engine = engine(&server);
    let first = hierarchy(engine.folder_hierarchy(None, None).await.unwrap());
    assert!(!first.more);
    let inbox_id = server.folder_id(None, "inbox").unwrap();
    let inbox = first
        .upserted
        .iter()
        .find(|f| f.remote_id == inbox_id)
        .unwrap();
    assert_eq!(inbox.name, "Inbox");
    assert_eq!(inbox.parent, None);
    assert_eq!(inbox.well_known, Some(ExchangeWellKnownFolder::Inbox));
    assert_eq!(inbox.total, Some(3));
    assert_eq!(inbox.unread, Some(2));
    let receipts = first
        .upserted
        .iter()
        .find(|f| f.name == "Receipts")
        .unwrap();
    assert_eq!(receipts.parent.as_deref(), Some(inbox_id.as_str()));
    assert_eq!(receipts.well_known, None);
    let archive = first.upserted.iter().find(|f| f.name == "Archive").unwrap();
    assert_eq!(archive.well_known, Some(ExchangeWellKnownFolder::Archive));
    let calendar = first
        .upserted
        .iter()
        .find(|f| f.name == "Calendar")
        .unwrap();
    assert_eq!(calendar.class, ExchangeFolderClass::Calendar);
    assert_eq!(calendar.well_known, Some(ExchangeWellKnownFolder::Calendar));
    let notes = first.upserted.iter().find(|f| f.name == "Notes").unwrap();
    assert_eq!(notes.class, ExchangeFolderClass::Other);
    assert!(
        !first
            .upserted
            .iter()
            .any(|f| f.well_known == Some(ExchangeWellKnownFolder::Root))
    );

    // Nothing changed.
    let quiet = hierarchy(
        engine
            .folder_hierarchy(None, Some(&first.cursor))
            .await
            .unwrap(),
    );
    assert!(quiet.upserted.is_empty() && quiet.removed.is_empty());

    let projects = server.add_folder(&inbox_id, "Projects", "IPF.Note");
    server.rename_folder(&receipts.remote_id, "Bills");
    let archive_id = archive.remote_id.clone();
    server.remove_folder(&archive_id);
    let changes = hierarchy(
        engine
            .folder_hierarchy(None, Some(&quiet.cursor))
            .await
            .unwrap(),
    );
    let names: Vec<&str> = changes.upserted.iter().map(|f| f.name.as_str()).collect();
    assert!(names.contains(&"Projects") && names.contains(&"Bills"));
    assert_eq!(changes.removed, [archive_id]);
    assert!(changes.upserted.iter().any(|f| f.remote_id == projects));

    server.expire_sync_states();
    assert_eq!(
        engine
            .folder_hierarchy(None, Some(&changes.cursor))
            .await
            .unwrap(),
        HierarchyOutcome::CursorExpired
    );
}

#[tokio::test]
async fn folder_items_full_and_incremental() {
    let server = MockEwsServer::start().await.unwrap();
    let engine = engine(&server);
    let inbox = server.folder_id(None, "inbox").unwrap();
    let full = delta(engine.folder_items(None, &inbox, None, None).await.unwrap());
    assert!(!full.more);
    assert_eq!(full.added.len(), 3);
    let numbers = full
        .added
        .iter()
        .find(|m| m.tags == ["Work"])
        .expect("the categorized message");
    assert!(numbers.flags.seen);
    assert_eq!(numbers.folder, inbox);
    assert!(numbers.thread_id.is_some());
    assert!(numbers.change_key.is_some());
    assert!(numbers.internal_date.is_some());
    assert!(numbers.size.unwrap() > 100);
    let info = numbers.exchange.as_ref().unwrap();
    assert_eq!(info.item_id, numbers.remote_id);
    assert_eq!(info.mailbox, None);
    assert_eq!(info.inference, None);
    let unread = full.added.iter().filter(|m| !m.flags.seen).count();
    assert_eq!(unread, 2);

    // `since` limits the first pass; the cursor is the same kind.
    let recent = delta(
        engine
            .folder_items(
                None,
                "inbox",
                None,
                nail_proto_ews::types::unix("2026-09-30T00:00:00Z"),
            )
            .await
            .unwrap(),
    );
    assert_eq!(recent.added.len(), 2);
    assert!(recent.added.iter().all(|m| m.folder == "inbox"));

    let quiet = delta(
        engine
            .folder_items(None, &inbox, Some(&full.cursor), None)
            .await
            .unwrap(),
    );
    assert!(quiet.added.is_empty() && quiet.changed.is_empty() && quiet.removed.is_empty());

    let raw = b"From: x@corp.example\r\nTo: ada@corp.example\r\nSubject: New\r\nMessage-ID: <new@corp.example>\r\n\r\nHello\r\n";
    let new_id = server.deliver(&inbox, raw, false);
    server.set_read(&numbers.remote_id, false);
    let unread_one = full.added.iter().find(|m| !m.flags.seen).unwrap();
    server.set_categories(&unread_one.remote_id, &["Personal"]);
    let gone = full
        .added
        .iter()
        .find(|m| m.flags.seen)
        .unwrap()
        .remote_id
        .clone();
    let third = full
        .added
        .iter()
        .find(|m| m.remote_id != numbers.remote_id && m.remote_id != unread_one.remote_id)
        .unwrap()
        .remote_id
        .clone();
    server.delete_item(&third);
    let changes = delta(
        engine
            .folder_items(None, &inbox, Some(&quiet.cursor), None)
            .await
            .unwrap(),
    );
    assert_eq!(changes.added.len(), 1);
    assert_eq!(changes.added[0].remote_id, new_id);
    assert_eq!(changes.removed, [third]);
    let read_change = changes
        .changed
        .iter()
        .find(|c| c.remote_id == gone)
        .expect("the read-state change");
    assert!(!read_change.flags.unwrap().seen);
    let tagged = changes
        .changed
        .iter()
        .find(|c| c.remote_id == unread_one.remote_id)
        .unwrap();
    assert_eq!(tagged.tags.as_deref(), Some(&["Personal".to_owned()][..]));
    assert!(tagged.change_key.is_some());

    // Paging.
    let paged_engine =
        EwsEngine::new(client(&server, EwsAuth::None), "work".into()).with_options(EwsOptions {
            page_size: 2,
            ..EwsOptions::default()
        });
    let page = delta(
        paged_engine
            .folder_items(None, &inbox, None, None)
            .await
            .unwrap(),
    );
    assert!(page.more);
    assert_eq!(page.added.len(), 2);
    let rest = delta(
        paged_engine
            .folder_items(None, &inbox, Some(&page.cursor), None)
            .await
            .unwrap(),
    );
    assert!(!rest.more);
    assert_eq!(rest.added.len(), 1);

    server.expire_sync_states();
    assert_eq!(
        engine
            .folder_items(None, &inbox, Some(&changes.cursor), None)
            .await
            .unwrap(),
        DeltaOutcome::CursorExpired
    );
    // A folder id that does not exist.
    let err = engine
        .folder_items(None, "AQMkNoSuchFolder", None, None)
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
}

#[tokio::test]
async fn mime_and_replayed_writes() {
    let server = MockEwsServer::start().await.unwrap();
    let engine = engine(&server);
    let inbox = server.folder_id(None, "inbox").unwrap();
    let items = server.items_in(&inbox);
    let first = items
        .iter()
        .find(|i| i.mime.as_deref() == Some(fixtures::INBOX_MESSAGES[0].as_bytes()))
        .unwrap();
    let mime = engine.fetch_mime(None, &first.id).await.unwrap();
    assert_eq!(mime, fixtures::INBOX_MESSAGES[0].as_bytes());
    assert_eq!(
        engine
            .fetch_mime(None, "AAMkMissing")
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );

    // Flags and categories.
    let before = server.item(&first.id).unwrap().change_key;
    let outcome = engine
        .write(
            None,
            &RemoteWrite::SetFlags {
                remote_id: first.id.clone(),
                change: FlagChange {
                    seen: Some(false),
                    flagged: Some(true),
                    answered: Some(true),
                    add_tags: vec!["Urgent".into(), "work".into()],
                    remove_tags: vec!["WORK".into()],
                    ..FlagChange::default()
                },
            },
        )
        .await
        .unwrap();
    assert_eq!(outcome, WriteOutcome::Done { remote_id: None });
    let item = server.item(&first.id).unwrap();
    assert_ne!(item.change_key, before);
    assert_eq!(item.props.child_text("IsRead"), Some("false"));
    assert_eq!(
        item.props.path(&["Flag", "FlagStatus"]).unwrap().text,
        "Flagged"
    );
    assert_eq!(item.props.strings("Categories", "String"), ["Urgent"]);
    assert!(
        item.props
            .children
            .iter()
            .any(|c| c.name == "ExtendedProperty")
    );
    let log = server.requests();
    let update = log
        .iter()
        .rev()
        .find(|r| r.operation == "UpdateItem")
        .unwrap();
    assert!(
        update
            .body
            .contains(r#"ConflictResolution="NeverOverwrite""#)
    );
    assert!(update.body.contains("ChangeKey="));

    // Flags only: no read first, overwrite.
    server.clear_requests();
    engine
        .write(
            None,
            &RemoteWrite::SetFlags {
                remote_id: first.id.clone(),
                change: FlagChange {
                    seen: Some(true),
                    ..FlagChange::default()
                },
            },
        )
        .await
        .unwrap();
    assert_eq!(server.operations(), ["UpdateItem"]);
    assert!(server.requests()[0].body.contains("AlwaysOverwrite"));

    // Unknown items are gone.
    assert_eq!(
        engine
            .write(
                None,
                &RemoteWrite::SetFlags {
                    remote_id: "AAMkMissing".into(),
                    change: FlagChange {
                        seen: Some(true),
                        ..FlagChange::default()
                    },
                },
            )
            .await
            .unwrap(),
        WriteOutcome::Gone
    );

    // Moves answer the new id.
    let receipts = server.folder_named(None, "Receipts").unwrap();
    let WriteOutcome::Done {
        remote_id: Some(moved),
    } = engine
        .write(
            None,
            &RemoteWrite::Move {
                remote_id: first.id.clone(),
                from_folder: inbox.clone(),
                to_folder: receipts.clone(),
            },
        )
        .await
        .unwrap()
    else {
        panic!("a move answers the new id");
    };
    assert_ne!(moved, first.id);
    assert_eq!(server.item(&moved).unwrap().folder, receipts);
    assert!(server.item(&first.id).is_none());
    assert_eq!(
        engine
            .write(
                None,
                &RemoteWrite::Move {
                    remote_id: first.id.clone(),
                    from_folder: inbox.clone(),
                    to_folder: receipts.clone(),
                },
            )
            .await
            .unwrap(),
        WriteOutcome::Gone
    );

    // Trash moves to Deleted Items; Delete removes.
    let WriteOutcome::Done {
        remote_id: Some(trashed),
    } = engine
        .write(None, &RemoteWrite::Trash { remote_id: moved })
        .await
        .unwrap()
    else {
        panic!("trash answers the new id");
    };
    let deleted_items = server.folder_id(None, "deleteditems").unwrap();
    assert_eq!(server.item(&trashed).unwrap().folder, deleted_items);
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
    assert!(server.item(&trashed).is_none());
    let delete = server
        .requests()
        .into_iter()
        .rev()
        .find(|r| r.operation == "DeleteItem")
        .unwrap();
    assert!(delete.body.contains(r#"DeleteType="HardDelete""#));
    assert_eq!(
        engine
            .write(None, &RemoteWrite::Delete { remote_id: trashed })
            .await
            .unwrap(),
        WriteOutcome::Gone
    );

    // Appends are idempotent (found again by Message-ID).
    let raw = b"From: ada@corp.example\r\nTo: bob@corp.example\r\nSubject: Copy\r\nMessage-ID: <copy-1@corp.example>\r\n\r\nText\r\n".to_vec();
    let append = RemoteWrite::Append {
        folder: "sentitems".into(),
        raw: raw.clone(),
        flags: Flags {
            seen: true,
            flagged: true,
            ..Flags::default()
        },
    };
    let WriteOutcome::Done {
        remote_id: Some(appended),
    } = engine.write(None, &append).await.unwrap()
    else {
        panic!("an append answers the id");
    };
    let item = server.item(&appended).unwrap();
    assert_eq!(item.mime.as_deref(), Some(raw.as_slice()));
    assert_eq!(item.props.child_text("IsDraft"), Some("false"));
    assert_eq!(item.props.child_text("IsRead"), Some("true"));
    assert_eq!(
        engine.write(None, &append).await.unwrap(),
        WriteOutcome::Done {
            remote_id: Some(appended.clone())
        }
    );
    assert_eq!(
        server
            .items_in(&server.folder_id(None, "sentitems").unwrap())
            .len(),
        1
    );

    // Copies (not a replayed write, but part of the client).
    let copy = engine
        .client()
        .move_item(
            None,
            &appended,
            nail_proto_ews::types::FolderRef::Id(&inbox),
            true,
        )
        .await
        .unwrap();
    assert!(server.item(&copy.id).is_some() && server.item(&appended).is_some());
}

#[tokio::test]
async fn change_keys_guard_updates() {
    let server = MockEwsServer::start().await.unwrap();
    let engine = engine(&server);
    let inbox = server.folder_id(None, "inbox").unwrap();
    let item = server.items_in(&inbox).remove(0);
    let stale = ItemRef::with_key(item.id.clone(), Some(item.change_key.clone()));
    server.set_property(&item.id, "Subject", "Changed elsewhere");
    let updates = nail_proto_ews::mail::flag_updates(&FlagChange {
        seen: Some(true),
        ..FlagChange::default()
    });
    let err = engine
        .client()
        .update_item(None, &stale, "Message", &updates, "")
        .await
        .unwrap_err();
    assert!(matches!(err, EwsError::Conflict(_)), "{err:?}");
    let current = server.item(&item.id).unwrap();
    let fresh = ItemRef::with_key(current.id.clone(), Some(current.change_key.clone()));
    let new_ref = engine
        .client()
        .update_item(None, &fresh, "Message", &updates, "")
        .await
        .unwrap();
    assert_ne!(new_ref.change_key, fresh.change_key);
}

#[tokio::test]
async fn sending_adds_bcc_and_saves_a_copy() {
    let server = MockEwsServer::start().await.unwrap();
    let engine = engine(&server);
    let raw = b"From: ada@corp.example\r\nTo: bob@corp.example\r\nSubject: Hi\r\nMessage-ID: <hi@corp.example>\r\n\r\nHi Bob\r\n";
    let receipt = engine
        .send(
            &Envelope {
                from: OWN_ADDRESS.into(),
                to: vec!["bob@corp.example".into(), "hidden@corp.example".into()],
            },
            raw,
        )
        .await
        .unwrap();
    assert!(receipt.response.contains("SendAndSaveCopy"));
    let sent = server.sent();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].saved);
    assert!(
        sent[0]
            .raw
            .starts_with(b"Bcc: hidden@corp.example\r\nFrom:")
    );
    let copies = server.items_in(&server.folder_id(None, "sentitems").unwrap());
    assert_eq!(copies.len(), 1);

    let no_copy =
        EwsEngine::new(client(&server, EwsAuth::None), "work".into()).with_options(EwsOptions {
            save_sent_copy: false,
            ..EwsOptions::default()
        });
    no_copy
        .send(
            &Envelope {
                from: OWN_ADDRESS.into(),
                to: vec!["bob@corp.example".into()],
            },
            raw,
        )
        .await
        .unwrap();
    assert!(!server.sent()[1].saved);
    let err = engine
        .send(
            &Envelope {
                from: OWN_ADDRESS.into(),
                to: vec![],
            },
            raw,
        )
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Rejected);
    server.inject(InjectedFault::code(
        "CreateItem",
        "ErrorMessageSizeExceeded",
    ));
    let err = engine
        .send(
            &Envelope {
                from: OWN_ADDRESS.into(),
                to: vec!["bob@corp.example".into()],
            },
            raw,
        )
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::TooLarge);
    assert!(!err.retryable);
}

#[tokio::test]
async fn shared_mailboxes_are_anchored() {
    let server = MockEwsServer::start().await.unwrap();
    let engine = engine(&server);
    let tree = hierarchy(
        engine
            .folder_hierarchy(Some(SHARED_ADDRESS), None)
            .await
            .unwrap(),
    );
    let inbox = tree
        .upserted
        .iter()
        .find(|f| f.well_known == Some(ExchangeWellKnownFolder::Inbox))
        .unwrap();
    let items = delta(
        engine
            .folder_items(Some(SHARED_ADDRESS), &inbox.remote_id, None, None)
            .await
            .unwrap(),
    );
    assert_eq!(items.added.len(), 1);
    assert_eq!(
        items.added[0].exchange.as_ref().unwrap().mailbox.as_deref(),
        Some(SHARED_ADDRESS)
    );
    let log = server.requests();
    let sync = log
        .iter()
        .find(|r| r.operation == "SyncFolderHierarchy")
        .unwrap();
    assert_eq!(sync.anchor.as_deref(), Some(SHARED_ADDRESS));
    assert!(sync.body.contains(SHARED_ADDRESS));
    // The private mailbox is closed; the delegate's mail is too.
    for closed in [
        nail_proto_ews::testing::PRIVATE_ADDRESS,
        nail_proto_ews::testing::DELEGATE_ADDRESS,
    ] {
        let err = engine
            .folder_hierarchy(Some(closed), None)
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::Forbidden, "{closed}");
    }
    let err = engine
        .folder_hierarchy(Some("nobody@corp.example"), None)
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
}

#[tokio::test]
async fn focused_inbox_is_read_when_enabled() {
    let server = MockEwsServer::start().await.unwrap();
    let inbox = server.folder_id(None, "inbox").unwrap();
    let id = server.items_in(&inbox)[0].id.clone();
    server.set_property(&id, "InferenceClassification", "Other");
    let plain = engine(&server);
    let err = plain
        .classify(
            None,
            std::slice::from_ref(&id),
            InferenceClassification::Focused,
            false,
        )
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Unsupported);
    assert_eq!(err.localized.unwrap().id, "ews-unsupported");
    let focused =
        EwsEngine::new(client(&server, EwsAuth::None), "work".into()).with_options(EwsOptions {
            focused_inbox: true,
            ..EwsOptions::default()
        });
    let items = delta(
        focused
            .folder_items(None, &inbox, None, None)
            .await
            .unwrap(),
    );
    let message = items.added.iter().find(|m| m.remote_id == id).unwrap();
    assert_eq!(
        message.exchange.as_ref().unwrap().inference,
        Some(InferenceClassification::Other)
    );
    focused
        .classify(
            None,
            std::slice::from_ref(&id),
            InferenceClassification::Focused,
            false,
        )
        .await
        .unwrap();
    assert_eq!(
        server
            .item(&id)
            .unwrap()
            .props
            .child_text("InferenceClassification"),
        Some("Focused")
    );
    let err = focused
        .classify(None, &[id], InferenceClassification::Other, true)
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Unsupported);
    assert!(
        server
            .requests()
            .iter()
            .any(|r| r.body.contains("item:InferenceClassification"))
    );
}

// ---------------------------------------------------------------------------
// Logins

#[derive(Debug, Default)]
struct Tokens {
    calls: AtomicU32,
    refreshes: AtomicU32,
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
            if force_refresh {
                self.refreshes.fetch_add(1, Ordering::SeqCst);
                Ok("fresh".to_owned())
            } else {
                Ok("stale".to_owned())
            }
        })
    }
}

#[tokio::test]
async fn basic_and_oauth_logins() {
    let server = MockEwsServer::start_with(MockAuth::Basic {
        username: "CORP\\ada".into(),
        password: "s3cret".into(),
    })
    .await
    .unwrap();
    let good = engine_with(&server, basic("s3cret"));
    good.server_info().await.unwrap();
    assert!(
        server
            .requests()
            .iter()
            .all(|r| r.auth_scheme.as_deref() == Some("Basic"))
    );
    let bad = engine_with(&server, basic("wrong"));
    let err = bad.server_info().await.unwrap_err();
    assert_eq!(err.code, ErrorCode::AuthFailed);
    assert!(!err.retryable);
    assert_eq!(err.localized.unwrap().id, "ews-auth-failed");

    // The keychain password is re-read once after a refusal.
    let store = Arc::new(MemorySecretStore::new());
    let secret = SecretRef::parse("keychain:nail/work/password").unwrap();
    store.set(&secret, "old").unwrap();
    let stored = engine_with(
        &server,
        EwsAuth::Basic {
            username: "CORP\\ada".into(),
            password: Password::Store {
                store: store.clone(),
                secret: secret.clone(),
            },
        },
    );
    assert_eq!(
        stored.server_info().await.unwrap_err().code,
        ErrorCode::AuthFailed
    );
    store.set(&secret, "s3cret").unwrap();
    stored.server_info().await.unwrap();

    // OAuth2: a stale token is refreshed once.
    let oauth_server = MockEwsServer::start_with(MockAuth::Bearer(vec!["fresh".into()]))
        .await
        .unwrap();
    let tokens = Arc::new(Tokens::default());
    let oauth = engine_with(
        &oauth_server,
        EwsAuth::OAuth2 {
            tokens: tokens.clone(),
            provider: "microsoft".into(),
            secret: SecretRef::parse("keychain:nail/work/oauth2").unwrap(),
        },
    );
    oauth.server_info().await.unwrap();
    assert!(tokens.refreshes.load(Ordering::SeqCst) >= 1);
    oauth_server.set_auth(MockAuth::Bearer(vec!["other".into()]));
    assert_eq!(
        oauth.server_info().await.unwrap_err().code,
        ErrorCode::AuthFailed
    );
}

fn basic(password: &str) -> EwsAuth {
    EwsAuth::Basic {
        username: "CORP\\ada".into(),
        password: Password::Fixed(password.into()),
    }
}

fn engine_with(server: &MockEwsServer, auth: EwsAuth) -> EwsEngine {
    EwsEngine::new(client(server, auth), "work".into())
}

#[tokio::test]
async fn negotiate_binds_to_the_connection() {
    let server = MockEwsServer::start_with(MockAuth::Negotiate)
        .await
        .unwrap();
    let engine = engine_with(
        &server,
        EwsAuth::Negotiate(Arc::new(ScriptedNegotiate::default())),
    );
    let inbox = server.folder_id(None, "inbox").unwrap();
    engine.server_info().await.unwrap();
    delta(engine.folder_items(None, &inbox, None, None).await.unwrap());
    let log = server.requests();
    // The handshake request carries the tokens; later calls on the same
    // connection need none.
    assert_eq!(log[0].auth_scheme.as_deref(), Some("Negotiate"));
    assert!(log.len() >= 3);
    let peer = log[0].peer;
    assert!(
        log.iter().all(|r| r.peer == peer),
        "one kept-alive connection"
    );
    assert!(log[1..].iter().all(|r| r.auth_scheme.is_none()));

    // The server forgets the connection: the client signs in again.
    server.forget_connections();
    server.clear_requests();
    delta(engine.folder_items(None, &inbox, None, None).await.unwrap());
    let log = server.requests();
    assert!(
        log.iter()
            .any(|r| r.auth_scheme.as_deref() == Some("Negotiate"))
    );

    // Streaming signs in on its own connection and does not block calls.
    let engine = Arc::new(engine);
    let waiting = {
        let engine = engine.clone();
        let folders = vec![inbox.clone()];
        tokio::spawn(async move { engine.wait_for_changes(None, &folders, 20).await })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    delta(engine.folder_items(None, &inbox, None, None).await.unwrap());
    server.deliver(&inbox, fixtures::RECEIPT.as_bytes(), false);
    let changed = tokio::time::timeout(Duration::from_secs(10), waiting)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(changed, Some(vec![inbox.clone()]));
    let peers: std::collections::HashSet<_> = server.requests().iter().map(|r| r.peer).collect();
    assert!(peers.len() >= 2, "a second connection for the stream");

    // Wrong credentials.
    let rejected = MockEwsServer::start_with(MockAuth::Negotiate)
        .await
        .unwrap();
    let engine = engine_with(
        &rejected,
        EwsAuth::Negotiate(Arc::new(ScriptedNegotiate { reject: true })),
    );
    assert_eq!(
        engine.server_info().await.unwrap_err().code,
        ErrorCode::AuthFailed
    );
}

// ---------------------------------------------------------------------------
// Throttling and errors

#[tokio::test]
async fn throttling_is_retried_or_reported() {
    let server = MockEwsServer::start().await.unwrap();
    let engine = engine(&server);
    let inbox = server.folder_id(None, "inbox").unwrap();
    // A short back-off is slept inline.
    server.inject(InjectedFault::busy(Some("SyncFolderItems"), 20));
    delta(engine.folder_items(None, &inbox, None, None).await.unwrap());
    // As a response message too.
    server.inject(InjectedFault {
        operation: Some("SyncFolderItems".into()),
        kind: FaultKind::ServerBusyMessage { back_off_ms: 10 },
        times: 1,
    });
    delta(engine.folder_items(None, &inbox, None, None).await.unwrap());
    // A long one goes to the caller with the wait.
    server.inject(InjectedFault::busy(Some("SyncFolderItems"), 120_000));
    let err = engine
        .folder_items(None, &inbox, None, None)
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::RateLimited);
    assert!(err.retryable);
    assert_eq!(err.retry_after_secs, Some(120));
    assert_eq!(err.localized.unwrap().id, "ews-rate-limited");
    // The recorded Exchange 2019 answer.
    server.answer_with("GetFolder", fixtures::RECORDED_SERVER_BUSY, 1);
    let err = engine
        .client()
        .get_folders(
            None,
            &[nail_proto_ews::types::FolderRef::distinguished(
                "inbox", None,
            )],
        )
        .await
        .unwrap_err();
    assert_eq!(err, EwsError::RateLimited(Some(298)));
    // HTTP 503 is retried for reads.
    server.inject(InjectedFault::http(Some("SyncFolderItems"), 503).times(2));
    delta(engine.folder_items(None, &inbox, None, None).await.unwrap());
    server.inject(InjectedFault::http(None, 503).times(4));
    let err = engine
        .folder_items(None, &inbox, None, None)
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Unavailable);
    assert!(err.retryable);
    server.inject(InjectedFault::http(None, 429).times(4));
    assert_eq!(
        engine
            .folder_items(None, &inbox, None, None)
            .await
            .unwrap_err()
            .code,
        ErrorCode::RateLimited
    );
    // Per-message errors.
    server.inject(InjectedFault::code("GetItem", "ErrorAccessDenied"));
    let id = server.items_in(&inbox)[0].id.clone();
    let err = engine.fetch_mime(None, &id).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::Forbidden);
    // A recorded invalid sync state is an expired cursor.
    server.answer_with("SyncFolderItems", fixtures::RECORDED_INVALID_SYNC_STATE, 1);
    assert_eq!(
        engine
            .folder_items(None, &inbox, Some("old-state"), None)
            .await
            .unwrap(),
        DeltaOutcome::CursorExpired
    );
}

// ---------------------------------------------------------------------------
// Streaming notifications

#[tokio::test]
async fn streaming_notifications_wake_the_wait() {
    let server = MockEwsServer::start().await.unwrap();
    let engine = Arc::new(engine(&server));
    let inbox = server.folder_id(None, "inbox").unwrap();
    let receipts = server.folder_named(None, "Receipts").unwrap();
    let folders = vec![inbox.clone(), receipts.clone()];

    // Nothing happens: an empty answer at the timeout.
    let quiet = engine.wait_for_changes(None, &folders, 1).await.unwrap();
    assert_eq!(quiet, Some(vec![]));
    let subscription = engine.watcher().subscription_id(None).unwrap();
    assert_eq!(server.subscriptions().len(), 1);

    // New mail wakes the wait.
    let waiting = {
        let engine = engine.clone();
        let folders = folders.clone();
        tokio::spawn(async move { engine.wait_for_changes(None, &folders, 20).await })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    server.deliver(&receipts, fixtures::RECEIPT.as_bytes(), false);
    let changed = tokio::time::timeout(Duration::from_secs(10), waiting)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(changed, Some(vec![receipts.clone()]));
    // The subscription is kept.
    assert_eq!(
        engine.watcher().subscription_id(None).unwrap(),
        subscription
    );

    // The server closes the stream and cuts it: the client reconnects.
    let waiting = {
        let engine = engine.clone();
        let folders = folders.clone();
        tokio::spawn(async move { engine.wait_for_changes(None, &folders, 20).await })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    server.close_streams();
    tokio::time::sleep(Duration::from_millis(300)).await;
    server.break_streams(1);
    tokio::time::sleep(Duration::from_millis(500)).await;
    let id = server.items_in(&inbox)[0].id.clone();
    server.set_read(&id, true);
    let changed = tokio::time::timeout(Duration::from_secs(10), waiting)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(changed, Some(vec![inbox.clone()]));
    let streams = server
        .operations()
        .into_iter()
        .filter(|o| o == "GetStreamingEvents")
        .count();
    assert!(streams >= 4, "{streams} streams");

    // A lost subscription is renewed and every folder may have changed.
    server.expire_subscriptions();
    let mut changed = engine
        .wait_for_changes(None, &folders, 5)
        .await
        .unwrap()
        .unwrap();
    changed.sort();
    let mut all = folders.clone();
    all.sort();
    assert_eq!(changed, all);
    assert_ne!(
        engine.watcher().subscription_id(None).unwrap(),
        subscription
    );
    // Lost again right away: an error the caller backs off from, not
    // another "everything may have changed" (no wake-all loop).
    server.expire_subscriptions();
    assert!(engine.wait_for_changes(None, &folders, 5).await.is_err());

    // Distinguished names work as folder names.
    let named = vec!["inbox".to_owned()];
    let waiting = {
        let engine = engine.clone();
        tokio::spawn(async move { engine.wait_for_changes(None, &named, 20).await })
    };
    tokio::time::sleep(Duration::from_millis(400)).await;
    server.deliver(&inbox, fixtures::RECEIPT.as_bytes(), false);
    let changed = tokio::time::timeout(Duration::from_secs(10), waiting)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(changed, Some(vec!["inbox".to_owned()]));
    let unsubscribed = server
        .operations()
        .iter()
        .filter(|o| *o == "Unsubscribe")
        .count();
    assert!(unsubscribed >= 1, "the old subscription is dropped");
}

#[tokio::test]
async fn streams_close_on_schedule_and_polling_engines_answer_none() {
    let server = MockEwsServer::start().await.unwrap();
    server.set_stream_close_after(Duration::from_millis(200));
    let engine = engine(&server);
    let inbox = server.folder_id(None, "inbox").unwrap();
    let folders = vec![inbox];
    assert_eq!(
        engine.wait_for_changes(None, &folders, 1).await.unwrap(),
        Some(vec![])
    );
    let streams = server
        .operations()
        .into_iter()
        .filter(|o| o == "GetStreamingEvents")
        .count();
    assert!(streams >= 2, "reopened after each close: {streams}");
    let polling =
        EwsEngine::new(client(&server, EwsAuth::None), "work".into()).with_options(EwsOptions {
            streaming: false,
            ..EwsOptions::default()
        });
    assert_eq!(
        polling.wait_for_changes(None, &folders, 1).await.unwrap(),
        None
    );
}

#[test]
fn recorded_stream_parses() {
    let mut buffer = fixtures::RECORDED_STREAM.to_owned();
    let documents = nail_proto_ews::soap::split_envelopes(&mut buffer);
    assert_eq!(documents.len(), 2);
    let mut events = Vec::new();
    let mut closed = false;
    for document in documents {
        let nail_proto_ews::soap::Answer::Response(response) =
            nail_proto_ews::soap::parse_answer(&document).unwrap()
        else {
            panic!("expected responses");
        };
        let batch = nail_proto_ews::notify::parse_stream_batch(&response.body).unwrap();
        events.extend(batch.events);
        closed |= batch.closed;
    }
    assert!(closed);
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].1.kind, "NewMailEvent");
    assert_eq!(events[0].1.parent_folder_id.as_deref(), Some("AQMkInbox"));
}

/// P3 (ADR 0164): a conversation's writes share multi-item requests; tag
/// changes and appends stay single calls.
#[tokio::test]
async fn many_writes_share_multi_item_requests() {
    let server = MockEwsServer::start().await.unwrap();
    let engine = engine(&server);
    let inbox = server.folder_id(None, "inbox").unwrap();
    let ids: Vec<String> = server.items_in(&inbox).into_iter().map(|i| i.id).collect();
    assert!(ids.len() >= 3, "{ids:?}");
    let receipts = server.folder_named(None, "Receipts").unwrap();
    let seen = FlagChange {
        seen: Some(true),
        ..FlagChange::default()
    };
    let writes = vec![
        RemoteWrite::SetFlags {
            remote_id: ids[0].clone(),
            change: seen.clone(),
        },
        RemoteWrite::SetFlags {
            remote_id: ids[1].clone(),
            change: seen.clone(),
        },
        RemoteWrite::SetFlags {
            remote_id: "AAMkMissing".into(),
            change: seen,
        },
        RemoteWrite::SetFlags {
            remote_id: ids[2].clone(),
            change: FlagChange {
                add_tags: vec!["Urgent".into()],
                ..FlagChange::default()
            },
        },
        RemoteWrite::Move {
            remote_id: ids[0].clone(),
            from_folder: inbox.clone(),
            to_folder: receipts.clone(),
        },
        RemoteWrite::Move {
            remote_id: ids[1].clone(),
            from_folder: inbox.clone(),
            to_folder: receipts.clone(),
        },
    ];
    server.clear_requests();
    let results: Vec<WriteOutcome> = engine
        .write_many(None, &writes)
        .await
        .into_iter()
        .map(Result::unwrap)
        .collect();
    assert_eq!(results[0], WriteOutcome::Done { remote_id: None });
    assert_eq!(results[2], WriteOutcome::Gone);
    let moved: Vec<String> = results[4..]
        .iter()
        .map(|r| match r {
            WriteOutcome::Done {
                remote_id: Some(id),
            } => id.clone(),
            other => panic!("{other:?}"),
        })
        .collect();
    for id in &moved {
        let item = server.item(id).unwrap();
        assert_eq!(item.folder, receipts);
        assert_eq!(item.props.child_text("IsRead"), Some("true"));
    }
    assert_eq!(
        server
            .item(&ids[2])
            .unwrap()
            .props
            .strings("Categories", "String"),
        ["Urgent"]
    );
    // One UpdateItem for the three reads, the tag change on its own (with
    // its read of the categories), one MoveItem for both moves.
    let operations = server.operations();
    assert_eq!(
        operations.iter().filter(|o| *o == "MoveItem").count(),
        1,
        "{operations:?}"
    );
    assert_eq!(
        operations.iter().filter(|o| *o == "UpdateItem").count(),
        2,
        "{operations:?}"
    );
    // Trash and permanent deletes.
    let trashed = engine
        .write_many(
            None,
            &[RemoteWrite::Trash {
                remote_id: moved[0].clone(),
            }],
        )
        .await;
    let Ok(WriteOutcome::Done {
        remote_id: Some(in_trash),
    }) = &trashed[0]
    else {
        panic!("{trashed:?}");
    };
    assert_eq!(
        server.item(in_trash).unwrap().folder,
        server.folder_id(None, "deleteditems").unwrap()
    );
    let deleted = engine
        .write_many(
            None,
            &[
                RemoteWrite::Delete {
                    remote_id: in_trash.clone(),
                },
                RemoteWrite::Delete {
                    remote_id: moved[1].clone(),
                },
                RemoteWrite::Delete {
                    remote_id: "AAMkMissing".into(),
                },
            ],
        )
        .await;
    assert_eq!(
        deleted,
        vec![
            Ok(WriteOutcome::Done { remote_id: None }),
            Ok(WriteOutcome::Done { remote_id: None }),
            Ok(WriteOutcome::Gone),
        ]
    );
    assert!(server.item(in_trash).is_none() && server.item(&moved[1]).is_none());
}
