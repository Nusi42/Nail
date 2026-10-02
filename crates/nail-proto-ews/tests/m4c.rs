//! M4c leftovers of the EWS engine against the mock (plan M4c WP123,
//! ADR 0094): send as / on behalf of shared mailboxes, headers-only
//! fetches (`PR_TRANSPORT_MESSAGE_HEADERS`), moves between mailboxes
//! (cross-mailbox `MoveItem`, or a MIME copy where the server refuses),
//! conversation ids as thread ids.

use std::time::Duration;

use nail_api::ErrorCode;
use nail_api::accounts::SendingMailbox;
use nail_api::domain::Flags;
use nail_api::engine::{DeltaOutcome, Envelope, OutgoingTransport};
use nail_api::exchange::engine::ExchangeMailEngine;
use nail_proto_ews::client::{CALL_TIMEOUT, lane_http};
use nail_proto_ews::mail::header_block;
use nail_proto_ews::testing::{MockEwsServer, OWN_ADDRESS, PRIVATE_ADDRESS, SHARED_ADDRESS};
use nail_proto_ews::{EwsAuth, EwsClient, EwsEngine, RetryPolicy};
use pretty_assertions::assert_eq;

fn engine(server: &MockEwsServer) -> EwsEngine {
    let client = EwsClient::with_http(
        lane_http(CALL_TIMEOUT).unwrap(),
        lane_http(Duration::from_secs(120)).unwrap(),
        &server.url(),
        EwsAuth::None,
    )
    .unwrap()
    .with_own_address(OWN_ADDRESS)
    .with_retry(RetryPolicy::fast());
    EwsEngine::new(client, "corp".into())
}

fn message(from: &str, sender: Option<&str>, subject: &str) -> Vec<u8> {
    let sender = sender.map_or_else(String::new, |s| format!("Sender: {s}\r\n"));
    format!(
        "From: {from}\r\n{sender}To: bob@corp.example\r\nSubject: {subject}\r\nMessage-ID: <{subject}@corp.example>\r\n\r\nBody of {subject}.\r\n"
    )
    .into_bytes()
}

fn to_bob() -> Envelope {
    Envelope {
        from: OWN_ADDRESS.into(),
        to: vec!["bob@corp.example".into()],
    }
}

#[tokio::test]
async fn send_as_and_on_behalf() {
    let server = MockEwsServer::start().await.unwrap();
    let engine = engine(&server);

    // The decision.
    assert_eq!(
        engine.sending_mailbox(&message("Ada <ADA@corp.example>", None, "x")),
        SendingMailbox::Own
    );
    assert_eq!(
        engine.sending_mailbox(&message(SHARED_ADDRESS, None, "x")),
        SendingMailbox::SendAs {
            mailbox: SHARED_ADDRESS.into()
        }
    );
    assert_eq!(
        engine.sending_mailbox(&message(
            &format!("Support <{SHARED_ADDRESS}>"),
            Some(&format!("\"Lovelace, Ada\" <{OWN_ADDRESS}>")),
            "x"
        )),
        SendingMailbox::OnBehalf {
            mailbox: SHARED_ADDRESS.into()
        }
    );

    engine
        .send(&to_bob(), &message(OWN_ADDRESS, None, "own-1"))
        .await
        .unwrap();
    engine
        .send(
            &to_bob(),
            &message(&format!("Support <{SHARED_ADDRESS}>"), None, "as-1"),
        )
        .await
        .unwrap();
    engine
        .send(
            &to_bob(),
            &message(SHARED_ADDRESS, Some(OWN_ADDRESS), "behalf-1"),
        )
        .await
        .unwrap();
    let sent = server.sent();
    assert_eq!(sent.len(), 3);
    assert_eq!(sent[0].from, None);
    assert_eq!(sent[0].saved_in.as_deref(), Some(OWN_ADDRESS));
    assert_eq!(sent[1].from.as_deref(), Some(SHARED_ADDRESS));
    assert_eq!(
        sent[1].saved_in.as_deref(),
        Some(SHARED_ADDRESS),
        "send as: the shared Sent Items"
    );
    assert_eq!(sent[2].from.as_deref(), Some(SHARED_ADDRESS));
    assert_eq!(
        sent[2].saved_in.as_deref(),
        Some(OWN_ADDRESS),
        "on behalf: the user's Sent Items"
    );
    let shared_sent = server.folder_id(Some(SHARED_ADDRESS), "sentitems").unwrap();
    assert!(
        server
            .items_in(&shared_sent)
            .iter()
            .any(
                |i| String::from_utf8_lossy(i.mime.as_deref().unwrap_or_default()).contains("as-1")
            )
    );
    let creates: Vec<_> = server
        .requests()
        .into_iter()
        .filter(|r| r.operation == "CreateItem")
        .collect();
    assert!(!creates[0].body.contains("From"), "{}", creates[0].body);
    assert!(creates[1].body.contains("From") && creates[1].body.contains(SHARED_ADDRESS));

    // A mailbox the user may not send as.
    let err = engine
        .send(&to_bob(), &message(PRIVATE_ADDRESS, None, "denied-1"))
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Forbidden);
    assert_eq!(server.sent().len(), 3);
}

#[tokio::test]
async fn headers_only_fetches() {
    let server = MockEwsServer::start().await.unwrap();
    let engine = engine(&server);
    let inbox = server.folder_id(None, "inbox").unwrap();
    let id = server.deliver(
        &inbox,
        b"Received: from mx.corp.example\r\n by ex.corp.example; Thu, 1 Oct 2026 08:00:00 +0000\r\nFrom: bob@corp.example\r\nTo: ada@corp.example\r\nSubject: Newsletter <&> news\r\nMessage-ID: <n1@corp.example>\r\n\r\nThe long body.\r\n",
        false,
    );
    let headers = String::from_utf8(engine.fetch_headers(None, &id).await.unwrap()).unwrap();
    assert!(
        headers.contains("Subject: Newsletter <&> news\r\n"),
        "{headers}"
    );
    assert!(headers.contains("Received: from mx.corp.example\r\n by ex.corp.example"));
    assert!(headers.ends_with("\r\n\r\n"));
    assert!(!headers.contains("long body"));
    let get = server
        .requests()
        .into_iter()
        .rfind(|r| r.operation == "GetItem")
        .unwrap();
    assert!(get.body.contains("0x007D") && !get.body.contains("IncludeMimeContent"));

    // Sent mail has no transport headers: the MIME is cut.
    let sent_folder = server.folder_id(None, "sentitems").unwrap();
    let sent = server.deliver(
        &sent_folder,
        b"From: ada@corp.example\r\nSubject: Mine\r\n\r\nText.\r\n",
        true,
    );
    let headers = engine.fetch_headers(None, &sent).await.unwrap();
    assert_eq!(headers, b"From: ada@corp.example\r\nSubject: Mine\r\n\r\n");
    // Gone.
    assert_eq!(
        nail_api::engine::EngineError::from(
            engine.fetch_headers(None, "AAMkGone").await.unwrap_err()
        )
        .code,
        ErrorCode::NotFound
    );
}

#[test]
fn header_blocks_are_safe() {
    assert_eq!(
        header_block(b"A: 1\nB: 2\n folded\nnot a header\n\nbody"),
        b"A: 1\r\nB: 2\r\n folded\r\n\r\n"
    );
    assert_eq!(header_block(b"A: 1"), b"A: 1\r\n\r\n");
    assert_eq!(header_block(b""), b"\r\n");
}

#[tokio::test]
async fn moves_between_mailboxes() {
    let server = MockEwsServer::start().await.unwrap();
    let engine = engine(&server);
    let inbox = server.folder_id(None, "inbox").unwrap();
    let shared_inbox = server.folder_id(Some(SHARED_ADDRESS), "inbox").unwrap();
    let id = server.deliver(&inbox, &message("bob@corp.example", None, "move-1"), true);

    // The server moves across mailboxes itself.
    let moved = engine
        .move_to_mailbox(None, &id, Some(SHARED_ADDRESS), "inbox", Flags::default())
        .await
        .unwrap();
    assert!(server.item(&id).is_none());
    assert_eq!(server.item(&moved).unwrap().folder, shared_inbox);
    assert!(server.requests().iter().any(|r| r.operation == "MoveItem"));

    // A server that refuses: a MIME copy with the flags, then a delete.
    server.allow_cross_mailbox_moves(false);
    server.clear_requests();
    let flags = Flags {
        seen: true,
        flagged: true,
        ..Flags::default()
    };
    let back = engine
        .move_to_mailbox(Some(SHARED_ADDRESS), &moved, None, "inbox", flags)
        .await
        .unwrap();
    assert!(server.item(&moved).is_none());
    let item = server.item(&back).unwrap();
    assert_eq!(item.folder, inbox);
    assert!(String::from_utf8_lossy(item.mime.as_deref().unwrap()).contains("Subject: move-1"));
    let ops: Vec<String> = server.requests().into_iter().map(|r| r.operation).collect();
    assert!(
        ops.contains(&"CreateItem".to_owned()) && ops.contains(&"DeleteItem".to_owned()),
        "{ops:?}"
    );

    // A mailbox without access: nothing is lost.
    let keep = server.deliver(&inbox, &message("bob@corp.example", None, "keep-1"), true);
    assert!(
        engine
            .move_to_mailbox(
                None,
                &keep,
                Some(PRIVATE_ADDRESS),
                "inbox",
                Flags::default()
            )
            .await
            .is_err()
    );
    assert!(server.item(&keep).is_some());
}

#[tokio::test]
async fn conversation_ids_are_thread_ids() {
    let server = MockEwsServer::start().await.unwrap();
    let engine = engine(&server);
    let DeltaOutcome::Delta(delta) = engine
        .folder_items(None, "inbox", None, None)
        .await
        .unwrap()
    else {
        panic!("delta");
    };
    assert!(!delta.added.is_empty());
    for m in &delta.added {
        let conversation = m.exchange.as_ref().and_then(|e| e.conversation_id.clone());
        assert_eq!(m.thread_id, conversation);
    }
    assert!(delta.added.iter().any(|m| m.thread_id.is_some()));
}
