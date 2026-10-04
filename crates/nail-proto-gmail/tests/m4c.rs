//! M4c (plan WP123, ADR 0094): Gmail thread ids are Nail's thread ids —
//! every path that reports a message (full listing, history, appends) must
//! carry the server's `threadId`, and replies the mock threads together
//! share it, so the core can map them into `messages.remote_thread_id`.
#![cfg(test)]

use nail_api::domain::Flags;
use nail_api::engine::{DeltaOutcome, MailApiEngine, RemoteMessage, RemoteWrite, WriteOutcome};
use nail_proto_gmail::testing::MockGmailServer;
use nail_proto_gmail::{GmailAuth, GmailClient, GmailEngine, RetryPolicy};
use pretty_assertions::assert_eq;

fn engine(server: &MockGmailServer) -> GmailEngine {
    GmailEngine::new(
        GmailClient::new(&server.api_base(), GmailAuth::Bearer("token".into()))
            .unwrap()
            .with_retry(RetryPolicy::fast()),
    )
}

async fn listing(engine: &GmailEngine) -> (String, Vec<RemoteMessage>) {
    let mut page = None;
    let mut cursor = None;
    let mut out = Vec::new();
    loop {
        let result = engine.full_sync_page(page.as_deref(), None).await.unwrap();
        cursor = cursor.or(result.cursor);
        out.extend(result.messages);
        match result.next_page {
            Some(next) => page = Some(next),
            None => break,
        }
    }
    (cursor.unwrap(), out)
}

fn message(id: &str, subject: &str, reply_to: Option<&str>) -> Vec<u8> {
    let threading = reply_to.map_or_else(String::new, |p| {
        format!("In-Reply-To: <{p}>\r\nReferences: <{p}>\r\n")
    });
    format!(
        "From: Grace <grace@example.org>\r\nTo: ada@gmail.example\r\nSubject: {subject}\r\nDate: Thu, 01 Oct 2026 11:00:00 +0000\r\nMessage-ID: <{id}>\r\n{threading}\r\nHello.\r\n"
    )
    .into_bytes()
}

#[tokio::test]
async fn every_listed_message_carries_its_gmail_thread() {
    let server = MockGmailServer::start_demo().await.unwrap();
    let engine = engine(&server);
    let (cursor, messages) = listing(&engine).await;
    assert!(!messages.is_empty());
    for m in &messages {
        let info = m.gmail.as_ref().expect("gmail data");
        assert_eq!(m.thread_id.as_deref(), Some(info.thread_id.as_str()));
        let server_thread = server.message(&m.remote_id).unwrap().thread_id;
        assert_eq!(info.thread_id, server_thread);
    }

    // A reply arrives later: history reports it in its thread.
    let first = server.deliver(
        &message("t-root@example.org", "Plans", None),
        &["INBOX", "UNREAD"],
    );
    let reply = server.deliver(
        &message(
            "t-reply@example.org",
            "Re: Plans",
            Some("t-root@example.org"),
        ),
        &["INBOX", "UNREAD"],
    );
    let root_thread = server.message(&first).unwrap().thread_id;
    assert_eq!(server.message(&reply).unwrap().thread_id, root_thread);
    let DeltaOutcome::Delta(delta) = engine.changes(&cursor).await.unwrap() else {
        panic!("delta");
    };
    let added: Vec<&RemoteMessage> = delta
        .added
        .iter()
        .filter(|m| m.remote_id == first || m.remote_id == reply)
        .collect();
    assert_eq!(added.len(), 2);
    assert!(
        added
            .iter()
            .all(|m| m.thread_id.as_deref() == Some(root_thread.as_str()))
    );

    // The thread call lists both.
    let ids = engine.thread_message_ids(&root_thread).await.unwrap();
    assert!(ids.contains(&first) && ids.contains(&reply));
}

#[tokio::test]
async fn appended_replies_join_their_thread() {
    let server = MockGmailServer::start_demo().await.unwrap();
    let engine = engine(&server);
    let root = server.deliver(&message("a-root@example.org", "Budget", None), &["INBOX"]);
    let outcome = engine
        .write(&RemoteWrite::Append {
            folder: "INBOX".into(),
            raw: message(
                "a-reply@example.org",
                "Re: Budget",
                Some("a-root@example.org"),
            ),
            flags: Flags::default(),
        })
        .await
        .unwrap();
    let WriteOutcome::Done {
        remote_id: Some(id),
    } = outcome
    else {
        panic!("{outcome:?}");
    };
    assert_eq!(
        server.message(&id).unwrap().thread_id,
        server.message(&root).unwrap().thread_id
    );
    let (_, messages) = listing(&engine).await;
    let listed = messages.iter().find(|m| m.remote_id == id).unwrap();
    assert_eq!(
        listed.thread_id.as_deref(),
        Some(server.message(&root).unwrap().thread_id.as_str())
    );
}
