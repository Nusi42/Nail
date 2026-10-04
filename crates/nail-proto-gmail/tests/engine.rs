//! The Gmail engine against the loopback mock Gmail server: full and
//! incremental sync, history expiry and slicing, raw fetches, replayed
//! writes, sending, labels, threads, tokens, quotas and backoff.
#![cfg(test)]

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use nail_api::ErrorCode;
use nail_api::accounts::SecretRef;
use nail_api::domain::{Flags, FolderRole};
use nail_api::engine::{
    AccessTokenProvider, BoxFuture, DeltaOutcome, EngineError, Envelope, MailApiEngine,
    OutgoingTransport, RemoteMessage, RemoteWrite, SyncDelta, WriteOutcome,
};
use nail_api::gmail::{GmailCategory, GmailLabelColor, GmailLabelKind, GmailLabelSettings};
use nail_api::ops::FlagChange;
use nail_proto_gmail::messages::Format;
use nail_proto_gmail::testing::fixtures::{DEMO_MAILBOX, DEMO_SYNCED_IDS};
use nail_proto_gmail::testing::{InjectedError, MockGmailServer};
use nail_proto_gmail::{
    EngineOptions, GmailAuth, GmailClient, GmailEngine, GmailError, PollTimer, RetryPolicy,
};
use pretty_assertions::assert_eq;

fn client(server: &MockGmailServer, auth: GmailAuth) -> GmailClient {
    GmailClient::new(&server.api_base(), auth)
        .unwrap()
        .with_retry(RetryPolicy::fast())
}

fn engine(server: &MockGmailServer) -> GmailEngine {
    GmailEngine::new(client(server, GmailAuth::Bearer("token".into())))
}

async fn demo() -> (MockGmailServer, GmailEngine) {
    let server = MockGmailServer::start_demo().await.unwrap();
    let engine = engine(&server);
    (server, engine)
}

async fn full_sync(engine: &GmailEngine, since: Option<i64>) -> (String, Vec<RemoteMessage>, u32) {
    let mut page = None;
    let mut cursor = None;
    let mut messages = Vec::new();
    let mut pages = 0;
    loop {
        let result = engine.full_sync_page(page.as_deref(), since).await.unwrap();
        if pages == 0 {
            cursor = result.cursor.clone();
        } else {
            assert_eq!(
                result.cursor, None,
                "only the first page carries the cursor"
            );
        }
        pages += 1;
        messages.extend(result.messages);
        match result.next_page {
            Some(next) => page = Some(next),
            None => break,
        }
    }
    (cursor.unwrap(), messages, pages)
}

async fn delta(engine: &GmailEngine, cursor: &str) -> SyncDelta {
    match engine.changes(cursor).await.unwrap() {
        DeltaOutcome::Delta(delta) => delta,
        DeltaOutcome::CursorExpired => panic!("cursor expired"),
    }
}

fn raw(message_id: &str, subject: &str, extra: &str) -> Vec<u8> {
    format!(
        "From: Grace <grace@example.org>\r\nTo: Ada <ada@gmail.example>\r\nSubject: {subject}\r\nDate: Thu, 01 Oct 2026 11:00:00 +0000\r\nMessage-ID: <{message_id}>\r\n{extra}MIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nHello.\r\n"
    )
    .into_bytes()
}

#[tokio::test]
async fn full_sync_pages_through_the_demo_mailbox() {
    let (server, engine) = demo().await;
    let engine = engine.with_options(EngineOptions {
        page_size: 3,
        batch_size: 2,
        ..EngineOptions::default()
    });
    let folders = engine.folders().await.unwrap();
    assert!(
        folders
            .iter()
            .any(|f| f.remote_id == "ARCHIVE" && f.role == FolderRole::Archive)
    );

    let (cursor, messages, pages) = full_sync(&engine, None).await;
    assert_eq!(cursor, "4200");
    assert_eq!(pages, 4, "10 messages in pages of 3");
    let mut ids: Vec<&str> = messages.iter().map(|m| m.remote_id.as_str()).collect();
    ids.sort_unstable();
    assert_eq!(
        ids, DEMO_SYNCED_IDS,
        "everything but the chat, Spam and Trash included"
    );
    // Newest first.
    assert_eq!(messages[0].remote_id, "18c0000000000009");

    let by_id = |id: &str| messages.iter().find(|m| m.remote_id == id).unwrap();
    let lunch = by_id("18c0000000000001");
    assert_eq!(lunch.folder, "INBOX");
    assert!(!lunch.flags.seen);
    assert_eq!(
        lunch.gmail.as_ref().unwrap().category,
        Some(GmailCategory::Primary)
    );
    assert_eq!(by_id("18c0000000000002").folder, "SENT");
    assert_eq!(
        by_id("18c0000000000002").thread_id.as_deref(),
        Some("18c0000000000001")
    );
    let notes = by_id("18c0000000000005");
    assert_eq!(notes.folder, "ARCHIVE");
    assert!(notes.flags.flagged && notes.flags.seen);
    assert_eq!(notes.tags, ["Work/Projects"]);
    assert_eq!(by_id("18c0000000000006").tags, ["Receipts"]);
    assert_eq!(by_id("18c0000000000007").folder, "SPAM");
    assert_eq!(by_id("18c0000000000008").folder, "TRASH");
    let draft = by_id("18c0000000000009");
    assert_eq!(draft.folder, "DRAFT");
    assert!(draft.flags.draft);
    assert_eq!(draft.internal_date, Some(1_790_850_600));
    assert!(draft.size.unwrap() > 100);

    // Metadata comes through batch requests, not one call per message.
    let requests = server.requests();
    let gets = requests
        .iter()
        .filter(|r| r.path.contains("/messages/18c"))
        .count();
    assert_eq!(gets, 10);
    assert!(
        requests
            .iter()
            .filter(|r| r.path.contains("/messages/18c"))
            .all(|r| r.in_batch)
    );
    assert!(requests.iter().all(|r| r.token.as_deref() == Some("token")));

    // "Recent N days": only messages after the cut-off.
    let (_, recent, _) = full_sync(&engine, Some(1_790_840_000)).await;
    let mut recent_ids: Vec<&str> = recent.iter().map(|m| m.remote_id.as_str()).collect();
    recent_ids.sort_unstable();
    assert_eq!(
        recent_ids,
        ["18c0000000000001", "18c0000000000002", "18c0000000000009"]
    );
}

#[tokio::test]
async fn history_brings_server_side_changes() {
    let (server, engine) = demo().await;
    let (cursor, _, _) = full_sync(&engine, None).await;

    let quiet = delta(&engine, &cursor).await;
    assert!(quiet.added.is_empty() && quiet.changed.is_empty() && quiet.removed.is_empty());
    assert_eq!(quiet.cursor, cursor);
    assert!(!quiet.more);

    // Server-side: new mail, read + archived, a new label on a message,
    // a permanent delete, a chat.
    let new_id = server.deliver(&raw("new-1@example.org", "News", ""), &["INBOX", "UNREAD"]);
    server.modify("18c0000000000001", &[], &["UNREAD", "INBOX"]);
    let travel = server.create_label("Travel");
    server.modify("18c0000000000004", &[&travel], &[]);
    server.delete("18c0000000000008");
    server.deliver(b"From: x@y\r\nSubject: chat\r\n\r\nhi", &["CHAT"]);

    let changes = delta(&engine, &cursor).await;
    assert_eq!(changes.cursor, server.history_id().to_string());
    assert_eq!(changes.added.len(), 1);
    assert_eq!(changes.added[0].remote_id, new_id);
    assert_eq!(changes.added[0].folder, "INBOX");
    assert_eq!(changes.removed, ["18c0000000000008"]);
    let lunch = changes
        .changed
        .iter()
        .find(|c| c.remote_id == "18c0000000000001")
        .unwrap();
    assert_eq!(lunch.folder.as_deref(), Some("ARCHIVE"));
    assert!(lunch.flags.unwrap().seen);
    let social = changes
        .changed
        .iter()
        .find(|c| c.remote_id == "18c0000000000004")
        .unwrap();
    assert_eq!(
        social.tags.as_deref(),
        Some(&["Travel".to_owned()][..]),
        "new labels are learned"
    );
    assert_eq!(changes.changed.len(), 2);

    // A reply joins the thread of its original.
    let reply = server.deliver(
        &raw(
            "new-2@example.org",
            "Re: News",
            "In-Reply-To: <new-1@example.org>\r\n",
        ),
        &["INBOX"],
    );
    let next = delta(&engine, &changes.cursor).await;
    assert_eq!(next.added[0].remote_id, reply);
    assert_eq!(next.added[0].thread_id.as_deref(), Some(new_id.as_str()));
}

#[tokio::test]
async fn expired_history_asks_for_a_full_resync() {
    let (server, engine) = demo().await;
    let (cursor, _, _) = full_sync(&engine, None).await;
    server.deliver(&raw("x@example.org", "x", ""), &["INBOX"]);
    server.expire_history();
    assert_eq!(
        engine.changes(&cursor).await.unwrap(),
        DeltaOutcome::CursorExpired
    );
    assert_eq!(
        engine.changes("not-a-number").await.unwrap(),
        DeltaOutcome::CursorExpired
    );
    let (fresh, messages, _) = full_sync(&engine, None).await;
    assert_eq!(messages.len(), 10);
    assert!(delta(&engine, &fresh).await.added.is_empty());
}

#[tokio::test]
async fn long_histories_are_read_in_slices() {
    let server = MockGmailServer::start("ada@gmail.example").await.unwrap();
    let engine = engine(&server).with_options(EngineOptions {
        history_page_size: 2,
        history_pages_per_call: 2,
        ..EngineOptions::default()
    });
    let (mut cursor, _, _) = full_sync(&engine, None).await;
    let mut ids = Vec::new();
    for i in 0..7 {
        ids.push(server.deliver(&raw(&format!("m{i}@x"), "x", ""), &["INBOX"]));
    }
    let mut seen = Vec::new();
    let mut calls = 0;
    loop {
        let d = delta(&engine, &cursor).await;
        calls += 1;
        seen.extend(d.added.into_iter().map(|m| m.remote_id));
        cursor = d.cursor;
        if !d.more {
            break;
        }
    }
    assert_eq!(calls, 2, "4 records per call");
    assert_eq!(seen, ids);
    assert_eq!(cursor, server.history_id().to_string());
}

#[tokio::test]
async fn raw_messages_include_attachments() {
    let (_server, engine) = demo().await;
    let bytes = engine.fetch_raw("18c0000000000005").await.unwrap();
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains("\r\nContent-Disposition: attachment; filename=\"note-g.pdf\""));
    // The recorded base64url message decodes byte for byte (UTF-8 euro).
    let receipt = engine.fetch_raw("18c0000000000006").await.unwrap();
    assert!(receipt.ends_with("12,50 €\r\n".as_bytes()));
    let err = engine.fetch_raw("18cfffffffffffff").await.unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
    assert_eq!(err.localized.unwrap().id, "gmail-missing");

    // The attachment API as well (`format=full` → attachmentId).
    let full = engine
        .client()
        .get_message("18c0000000000005", Format::Full, &[])
        .await
        .unwrap();
    let part = &full.payload.unwrap().parts[0];
    assert_eq!(part.filename, "note-g.pdf");
    let data = engine
        .client()
        .attachment(
            "18c0000000000005",
            part.body
                .as_ref()
                .unwrap()
                .attachment_id
                .as_deref()
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(data.starts_with(b"%PDF-1.4"));
    let meta = engine
        .client()
        .get_message("18c0000000000001", Format::Metadata, &["Message-ID"])
        .await
        .unwrap();
    assert_eq!(meta.header("message-id"), Some("<lunch-1@example.org>"));
}

#[tokio::test]
async fn flag_and_tag_writes_become_label_changes() {
    let (server, engine) = demo().await;
    let id = "18c0000000000001".to_owned();
    let outcome = engine
        .write(&RemoteWrite::SetFlags {
            remote_id: id.clone(),
            change: FlagChange {
                seen: Some(true),
                flagged: Some(true),
                answered: Some(true),
                add_tags: vec!["Work/Projects".into(), "Brand New".into()],
                ..FlagChange::default()
            },
        })
        .await
        .unwrap();
    assert_eq!(outcome, WriteOutcome::Done { remote_id: None });
    let labels = server.labels_of(&id);
    assert!(!labels.contains(&"UNREAD".to_owned()));
    assert!(labels.contains(&"STARRED".to_owned()));
    assert!(labels.contains(&"Label_1".to_owned()));
    let created = server
        .labels()
        .into_iter()
        .find(|l| l.name == "Brand New")
        .expect("a new tag creates its label");
    assert!(labels.contains(&created.id));

    // Replaying is harmless; removing tags removes labels; unknown tags
    // to remove are ignored.
    engine
        .write(&RemoteWrite::SetFlags {
            remote_id: id.clone(),
            change: FlagChange {
                seen: Some(false),
                remove_tags: vec!["Brand New".into(), "Never Existed".into()],
                ..FlagChange::default()
            },
        })
        .await
        .unwrap();
    let labels = server.labels_of(&id);
    assert!(labels.contains(&"UNREAD".to_owned()));
    assert!(!labels.contains(&created.id));
    // An empty change sends nothing.
    server.clear_requests();
    engine
        .write(&RemoteWrite::SetFlags {
            remote_id: id.clone(),
            change: FlagChange {
                answered: Some(true),
                ..FlagChange::default()
            },
        })
        .await
        .unwrap();
    assert!(
        server
            .requests()
            .iter()
            .all(|r| !r.path.ends_with("/modify"))
    );

    let gone = engine
        .write(&RemoteWrite::SetFlags {
            remote_id: "18cfffffffffffff".into(),
            change: FlagChange {
                seen: Some(true),
                ..FlagChange::default()
            },
        })
        .await
        .unwrap();
    assert_eq!(gone, WriteOutcome::Gone);
}

#[tokio::test]
async fn moves_trash_and_delete() {
    let (server, engine) = demo().await;
    let id = "18c0000000000003";
    let mv = |from: &str, to: &str| RemoteWrite::Move {
        remote_id: id.to_owned(),
        from_folder: from.to_owned(),
        to_folder: to.to_owned(),
    };
    let folder = |server: &MockGmailServer| {
        nail_api::gmail::GmailFolder::for_labels(&server.labels_of(id)).path()
    };
    engine.write(&mv("INBOX", "ARCHIVE")).await.unwrap();
    assert_eq!(folder(&server), "ARCHIVE");
    assert!(
        server
            .labels_of(id)
            .contains(&"CATEGORY_PROMOTIONS".to_owned())
    );
    engine.write(&mv("ARCHIVE", "INBOX")).await.unwrap();
    assert_eq!(folder(&server), "INBOX");
    engine.write(&mv("INBOX", "SPAM")).await.unwrap();
    assert_eq!(folder(&server), "SPAM");
    engine.write(&mv("SPAM", "TRASH")).await.unwrap();
    assert_eq!(folder(&server), "TRASH");
    let paths: Vec<String> = server.requests().iter().map(|r| r.path.clone()).collect();
    assert!(paths.iter().any(|p| p.ends_with("/trash")));
    engine.write(&mv("TRASH", "INBOX")).await.unwrap();
    assert_eq!(folder(&server), "INBOX");
    assert!(
        server
            .requests()
            .iter()
            .any(|r| r.path.ends_with("/untrash"))
    );
    // Replays are idempotent.
    engine.write(&mv("TRASH", "INBOX")).await.unwrap();
    assert_eq!(folder(&server), "INBOX");

    let err = engine.write(&mv("INBOX", "SENT")).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::Unsupported);
    assert!(!err.retryable);
    assert_eq!(err.localized.unwrap().id, "gmail-move-unsupported");
    assert!(engine.write(&mv("INBOX", "Elsewhere")).await.is_err());

    engine
        .write(&RemoteWrite::Trash {
            remote_id: id.to_owned(),
        })
        .await
        .unwrap();
    assert_eq!(folder(&server), "TRASH");
    engine
        .write(&RemoteWrite::Delete {
            remote_id: id.to_owned(),
        })
        .await
        .unwrap();
    assert!(server.message(id).is_none());
    for write in [
        RemoteWrite::Delete {
            remote_id: id.to_owned(),
        },
        RemoteWrite::Trash {
            remote_id: id.to_owned(),
        },
        mv("INBOX", "ARCHIVE"),
        mv("TRASH", "INBOX"),
    ] {
        assert_eq!(
            engine.write(&write).await.unwrap(),
            WriteOutcome::Gone,
            "{write:?}"
        );
    }
}

#[tokio::test]
async fn appends_are_idempotent_and_drafts_use_the_drafts_api() {
    let (server, engine) = demo().await;
    let message = raw(
        "append-1@x",
        "Imported",
        "In-Reply-To: <lunch-1@example.org>\r\n",
    );
    let append = RemoteWrite::Append {
        folder: "INBOX".into(),
        raw: message.clone(),
        flags: Flags {
            seen: false,
            flagged: true,
            ..Flags::default()
        },
    };
    let WriteOutcome::Done {
        remote_id: Some(first),
    } = engine.write(&append).await.unwrap()
    else {
        panic!("expected an id");
    };
    let stored = server.message(&first).unwrap();
    assert_eq!(stored.raw, message);
    assert_eq!(stored.label_ids, ["INBOX", "UNREAD", "STARRED"]);
    assert_eq!(
        stored.thread_id, "18c0000000000001",
        "filed into the original's thread"
    );
    assert_eq!(
        stored.internal_date_ms, 1_790_852_400_000,
        "date from the header"
    );
    let WriteOutcome::Done {
        remote_id: Some(again),
    } = engine.write(&append).await.unwrap()
    else {
        panic!("expected an id");
    };
    assert_eq!(again, first, "a replayed append finds the stored copy");

    let draft = RemoteWrite::Append {
        folder: "DRAFT".into(),
        raw: raw("draft-2@x", "Draft", ""),
        flags: Flags::default(),
    };
    let WriteOutcome::Done {
        remote_id: Some(draft_id),
    } = engine.write(&draft).await.unwrap()
    else {
        panic!("expected an id");
    };
    assert_eq!(server.labels_of(&draft_id), ["DRAFT"]);
    assert!(
        server
            .requests()
            .iter()
            .any(|r| r.path.ends_with("/drafts"))
    );

    let archive = RemoteWrite::Append {
        folder: "ARCHIVE".into(),
        raw: raw("archived@x", "Old", ""),
        flags: Flags {
            seen: true,
            ..Flags::default()
        },
    };
    let WriteOutcome::Done {
        remote_id: Some(archived),
    } = engine.write(&archive).await.unwrap()
    else {
        panic!("expected an id");
    };
    assert!(server.labels_of(&archived).is_empty());
}

#[tokio::test]
async fn sending_goes_through_the_api() {
    let (server, engine) = demo().await;
    let message = raw(
        "out-1@gmail.example",
        "Re: Lunch on Friday?",
        "In-Reply-To: <lunch-1@example.org>\r\n",
    );
    let receipt = engine
        .send(
            &Envelope {
                from: "ada@gmail.example".into(),
                to: vec!["ada@gmail.example".into(), "secret@example.org".into()],
            },
            &message,
        )
        .await
        .unwrap();
    assert!(receipt.response.starts_with("gmail id "));
    let sent = server.sent();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].starts_with(b"Bcc: secret@example.org\r\n"));
    let stored = server
        .with_mailbox(|m| {
            m.message_ids()
                .into_iter()
                .filter_map(|id| m.message(&id).cloned())
                .find(|msg| msg.raw.ends_with(b"Hello.\r\n") && msg.label_ids == ["SENT"])
        })
        .unwrap();
    assert_eq!(stored.thread_id, "18c0000000000001");
    assert!(
        !String::from_utf8_lossy(&stored.raw).contains("secret@"),
        "Gmail drops Bcc"
    );

    // No recipients in the headers or the envelope: Gmail refuses.
    let err = engine
        .send(
            &Envelope {
                from: "ada@gmail.example".into(),
                to: vec![],
            },
            b"Subject: nobody\r\n\r\nx",
        )
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Rejected);
    assert!(!err.retryable);

    // Large messages go through the media upload.
    let mut big = raw("big@gmail.example", "Big", "");
    big.extend(std::iter::repeat_n(b'a', 5 * 1024 * 1024));
    big.extend_from_slice(b"\r\n");
    let sent = engine.send_raw(&big).await.unwrap();
    assert_eq!(server.message(&sent.id).unwrap().raw.len(), big.len());
    assert!(
        server
            .requests()
            .iter()
            .any(|r| r.path == "/upload/gmail/v1/users/me/messages/send"
                && r.query.contains("uploadType=multipart"))
    );
}

#[tokio::test]
async fn sending_is_not_repeated_after_a_server_error() {
    let (server, engine) = demo().await;
    server.inject(InjectedError::backend_error().on("/messages/send").times(5));
    let err = engine.send_raw(&raw("s@x", "s", "")).await.unwrap_err();
    assert!(matches!(err, GmailError::Unavailable { status: 500, .. }));
    assert_eq!(
        server
            .requests()
            .iter()
            .filter(|r| r.path.ends_with("/messages/send"))
            .count(),
        1
    );
    let engine_err: EngineError = err.into();
    assert!(engine_err.retryable);
}

#[tokio::test]
async fn labels_are_managed_through_the_api() {
    let (server, engine) = demo().await;
    let labels = engine.labels().await.unwrap();
    assert_eq!(labels[0].kind, GmailLabelKind::System);
    let users: Vec<&str> = labels
        .iter()
        .filter(|l| l.kind == GmailLabelKind::User)
        .map(|l| l.settings.name.as_str())
        .collect();
    assert_eq!(users, ["Receipts", "Work/Projects"]);
    let inbox = labels.iter().find(|l| l.id == "INBOX").unwrap();
    assert_eq!(inbox.folder, Some(nail_api::gmail::GmailFolder::Inbox));

    let mut settings = GmailLabelSettings::named("Family");
    settings.color = Some(GmailLabelColor {
        background: "#16A766".into(),
        text: "#ffffff".into(),
    });
    let created = engine.create_label(&settings).await.unwrap();
    assert!(created.id.starts_with("Label_"));
    assert_eq!(created.tag(), Some("Family"));
    // Replaying the create returns the label instead of failing.
    let again = engine
        .create_label(&GmailLabelSettings::named("family"))
        .await
        .unwrap();
    assert_eq!(again.id, created.id);

    let mut bad = GmailLabelSettings::named("Odd colour");
    bad.color = Some(GmailLabelColor {
        background: "#123456".into(),
        text: "#ffffff".into(),
    });
    let err = engine.create_label(&bad).await.unwrap_err();
    assert!(matches!(err, GmailError::ColorRejected(_)), "{err:?}");
    assert_eq!(err.localized().id, "gmail-label-color-rejected");

    let mut renamed = created.settings.clone();
    renamed.name = "Family/Kids".into();
    renamed.color = None;
    let updated = engine.update_label(&created.id, &renamed).await.unwrap();
    assert_eq!(updated.settings.name, "Family/Kids");
    assert_eq!(updated.settings.color, None);

    server.modify("18c0000000000004", &[&created.id], &[]);
    let counted = engine.label(&created.id).await.unwrap();
    assert_eq!(counted.messages_total, Some(1));
    assert_eq!(counted.messages_unread, Some(0));

    engine.delete_label(&created.id).await.unwrap();
    engine.delete_label(&created.id).await.unwrap();
    assert!(!server.labels_of("18c0000000000004").contains(&created.id));
    assert!(matches!(
        engine
            .update_label("INBOX", &GmailLabelSettings::named("x"))
            .await,
        Err(GmailError::Rejected(_))
    ));
}

#[tokio::test]
async fn threads_list_and_get() {
    let (_server, engine) = demo().await;
    let ids = engine.thread_message_ids("18c0000000000001").await.unwrap();
    assert_eq!(ids, ["18c0000000000001", "18c0000000000002"]);
    let page = engine
        .client()
        .list_threads(&nail_proto_gmail::messages::ListQuery {
            label_ids: vec!["INBOX".into()],
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(page.threads.len(), 4);
    assert!(matches!(
        engine.client().get_thread("nope", Format::Minimal).await,
        Err(GmailError::NotFound(_))
    ));
    let one = engine.message("18c0000000000006").await.unwrap();
    assert_eq!(one.tags, ["Receipts"]);
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
            assert_eq!(provider, "google");
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.revoked {
                return Err(EngineError::permanent(ErrorCode::AuthFailed, "revoked"));
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

fn oauth(tokens: Arc<Tokens>) -> GmailAuth {
    GmailAuth::OAuth2 {
        tokens,
        provider: "google".into(),
        secret: SecretRef::parse("keychain:nail/gmail/oauth2").unwrap(),
    }
}

#[tokio::test]
async fn a_refused_token_is_refreshed_once() {
    let server = MockGmailServer::start_demo().await.unwrap();
    server.set_token(Some("fresh"));
    let tokens = Arc::new(Tokens::default());
    let engine = GmailEngine::new(client(&server, oauth(tokens.clone())));
    let profile = engine.profile().await.unwrap();
    assert_eq!(profile.email_address, "ada@gmail.example");
    assert_eq!(tokens.forced.load(Ordering::SeqCst), 1);

    // A token the server never accepts: AuthFailed after one refresh.
    server.set_token(Some("other"));
    let err = engine.profile().await.unwrap_err();
    assert_eq!(err, GmailError::AuthFailed);
    assert_eq!(tokens.forced.load(Ordering::SeqCst), 2);
    let engine_err: EngineError = err.into();
    assert_eq!(engine_err.code, ErrorCode::AuthFailed);
    assert!(!engine_err.retryable);

    // A revoked refresh token is the provider's error, unchanged.
    let revoked = Arc::new(Tokens {
        revoked: true,
        ..Tokens::default()
    });
    let engine = GmailEngine::new(client(&server, oauth(revoked)));
    let err = engine.folders().await.map(drop);
    assert!(err.is_ok(), "folders need no request");
    let err: EngineError = engine.full_sync_page(None, None).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::AuthFailed);
    assert_eq!(err.message, "revoked");

    // A fixed bearer token is not refreshed.
    let fixed = GmailEngine::new(client(&server, GmailAuth::Bearer("wrong".into())));
    assert_eq!(fixed.profile().await.unwrap_err(), GmailError::AuthFailed);
}

#[tokio::test]
async fn quotas_and_server_errors_back_off() {
    let (server, engine) = demo().await;
    // Retry-After within the inline limit: retried transparently.
    server.inject(InjectedError::rate_limit(Some(0)).on("/profile").times(2));
    engine.profile().await.unwrap();
    assert_eq!(
        server
            .requests()
            .iter()
            .filter(|r| r.path.ends_with("/profile"))
            .count(),
        3
    );

    // 5xx and 403 userRateLimitExceeded: exponential backoff, then success.
    server.inject(InjectedError::backend_error().on("/labels").times(2));
    server.inject(InjectedError::user_rate_limit().on("/labels").times(1));
    engine.labels().await.unwrap();

    // Retry-After beyond the inline limit: the caller gets it.
    server.inject(InjectedError::rate_limit(Some(120)).on("/history"));
    let err = engine.changes("4200").await.unwrap_err();
    assert_eq!(err.code, ErrorCode::RateLimited);
    assert_eq!(err.retry_after_secs, Some(120));
    assert!(err.retryable);
    let mut timer = PollTimer::new(&nail_api::gmail::GmailOptions::default());
    timer.observe(&Err(err));
    assert_eq!(timer.next_delay(), std::time::Duration::from_mins(2));

    // Retries run out.
    // 1 + 3 retries of RetryPolicy::fast.
    server.inject(InjectedError::status(503).on("/profile").times(4));
    let err = engine.profile().await.unwrap_err();
    assert!(matches!(err, GmailError::Unavailable { status: 503, .. }));

    // Not retried: insufficient scope.
    server.clear_requests();
    server.inject(InjectedError::insufficient_scope().on("/labels"));
    assert_eq!(
        engine.labels().await.unwrap_err(),
        GmailError::InsufficientScope
    );
    assert_eq!(server.requests().len(), 1);

    // The quota in Gmail units.
    server.set_quota(Some(6), 300);
    engine.profile().await.unwrap();
    engine
        .client()
        .get_message("18c0000000000001", Format::Minimal, &[])
        .await
        .unwrap();
    let err: EngineError = engine.fetch_raw("18c0000000000001").await.unwrap_err();
    assert_eq!(err.code, ErrorCode::RateLimited);
    assert_eq!(err.retry_after_secs, Some(300));
    server.set_quota(None, 0);
}

#[tokio::test]
async fn batch_parts_are_retried_on_their_own() {
    let (server, engine) = demo().await;
    // One part rate limited, then fine; one part gone for good.
    server.inject(
        InjectedError::rate_limit(Some(0))
            .on("/messages/18c0000000000003")
            .times(2),
    );
    let ids: Vec<String> = [
        "18c0000000000001",
        "18c0000000000003",
        "18cfffffffffffff",
        "bad/id",
    ]
    .map(str::to_owned)
    .to_vec();
    let answers = engine
        .client()
        .batch_get(&ids, Format::Minimal, 50)
        .await
        .unwrap();
    assert_eq!(answers[0].as_ref().unwrap().id, "18c0000000000001");
    assert_eq!(answers[1].as_ref().unwrap().id, "18c0000000000003");
    assert!(matches!(answers[2], Err(GmailError::NotFound(_))));
    assert!(matches!(answers[3], Err(GmailError::NotFound(_))));
    let tries = server
        .requests()
        .iter()
        .filter(|r| r.path.ends_with("/messages/18c0000000000001"))
        .count();
    assert_eq!(tries, 1, "only the limited part is sent again");

    // A part that stays limited fails the listing (the cursor is kept).
    server.inject(
        InjectedError::rate_limit(Some(60))
            .on("/messages/18c0000000000003")
            .times(10),
    );
    let err = engine.full_sync_page(None, None).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::RateLimited);
    assert_eq!(err.retry_after_secs, Some(60));

    // The batch request itself failing.
    server.inject(InjectedError::status(400).on("/batch"));
    assert!(
        engine
            .client()
            .batch_get(&ids[..1], Format::Minimal, 50)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn engine_for_a_configured_account() {
    use nail_api::accounts::{AuthConfig, IncomingServer, ServerEndpoint, TlsMode};
    let server = MockGmailServer::start_demo().await.unwrap();
    server.set_token(Some("fresh"));
    let incoming = IncomingServer::Gmail {
        server: ServerEndpoint {
            host: "127.0.0.1".into(),
            port: server.addr().port(),
            tls: TlsMode::Plaintext,
            auth: AuthConfig::OAuth2 {
                username: "ada@gmail.example".into(),
                provider: "google".into(),
                secret: None,
                mechanism: None,
            },
        },
        options: nail_api::gmail::GmailOptions::default(),
    };
    let mut account: nail_api::AccountConfig = serde_json::from_value(serde_json::json!({
        "id": "gm", "name": "Gmail", "email": "ada@gmail.example",
        "incoming": serde_json::to_value(&incoming).unwrap(),
    }))
    .unwrap();
    let tokens = Arc::new(Tokens::default());
    let engine = GmailEngine::for_account(&account, tokens).unwrap();
    assert_eq!(engine.profile().await.unwrap().history_id, "4200");

    if let IncomingServer::Gmail {
        server: endpoint, ..
    } = &mut account.incoming
    {
        endpoint.auth = AuthConfig::None;
    }
    assert!(matches!(
        GmailEngine::for_account(&account, Arc::new(Tokens::default())),
        Err(GmailError::Config(_))
    ));
    // Plain http to a non-loopback host is refused.
    if let IncomingServer::Gmail {
        server: endpoint, ..
    } = &mut account.incoming
    {
        endpoint.host = "gmail.example".into();
        endpoint.auth = AuthConfig::OAuth2 {
            username: "a".into(),
            provider: "google".into(),
            secret: None,
            mechanism: None,
        };
    }
    assert!(matches!(
        GmailEngine::for_account(&account, Arc::new(Tokens::default())),
        Err(GmailError::InsecureUrl(_))
    ));
}

#[tokio::test]
async fn mock_server_fixtures_and_loopback() {
    let server = MockGmailServer::start_fixture(DEMO_MAILBOX).await.unwrap();
    assert!(server.addr().ip().is_loopback());
    assert!(MockGmailServer::start_fixture("{}").await.is_err());
    let body = reqwest_get(&server, "/messages?q=frobnicate:1").await;
    assert_eq!(body.0, 400);
    let body = reqwest_get(&server, "/history").await;
    assert_eq!(body.0, 400, "startHistoryId is required");
    let body = reqwest_get(&server, "/nothing/here").await;
    assert_eq!(body.0, 404);
}

async fn reqwest_get(server: &MockGmailServer, path: &str) -> (u16, String) {
    let http = nail_auth::http_client().unwrap();
    let response = http
        .get(format!("{}{path}", server.api_base()))
        .bearer_auth("t")
        .send()
        .await
        .unwrap();
    (response.status().as_u16(), response.text().await.unwrap())
}

#[tokio::test]
async fn every_message_id_is_listed_without_fetching() {
    // Review M3b: merging a resumed full listing needs the server's ids.
    let (server, engine) = demo().await;
    let trashed = server.deliver(&raw("t@example.org", "t", ""), &["TRASH"]);
    let (_, listed, _) = full_sync(&engine, None).await;
    server.clear_requests();
    let ids = engine.all_message_ids().await.unwrap();
    let expected: std::collections::HashSet<String> =
        listed.iter().map(|m| m.remote_id.clone()).collect();
    // Every listed message (the listing leaves out chat transcripts, the
    // id set does not: they are never stored, so the merge ignores them).
    assert!(expected.is_subset(&ids), "{expected:?} {ids:?}");
    assert!(ids.contains(&trashed));
    // Ids only: no message was fetched.
    assert!(
        server
            .requests()
            .iter()
            .all(|r| r.path.ends_with("/messages") && !r.in_batch),
        "{:?}",
        server.requests()
    );
}
