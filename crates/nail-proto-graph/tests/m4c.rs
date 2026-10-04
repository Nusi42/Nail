//! M4c leftovers of the Graph engine against the mock (plan M4c WP123,
//! ADR 0094): upload sessions for messages over 4 MB (sending and
//! appending, progress, retries, clean-up), send as / on behalf of shared
//! mailboxes, headers-only fetches, moves between mailboxes, conversation
//! ids as thread ids.
#![cfg(test)]

use std::sync::Mutex;

use base64::Engine as _;
use nail_api::ErrorCode;
use nail_api::domain::Flags;
use nail_api::engine::{DeltaOutcome, Envelope, OutgoingTransport, RemoteWrite, WriteOutcome};
use nail_api::exchange::engine::ExchangeMailEngine;
use nail_proto_graph::testing::{InjectedError, MockGraphServer};
use nail_proto_graph::upload::{DraftParts, UPLOAD_UNIT};
use nail_proto_graph::{EngineOptions, GraphAuth, GraphClient, GraphEngine, RetryPolicy};
use pretty_assertions::assert_eq;

const ME: &str = "ada@contoso.example";
const SHARED: &str = "support@contoso.example";

fn engine_for(server: &MockGraphServer) -> GraphEngine {
    let client = GraphClient::with_http(
        nail_auth::http_client().unwrap(),
        &server.api_base(),
        GraphAuth::Bearer("token".into()),
    )
    .unwrap()
    .with_retry(RetryPolicy::fast());
    GraphEngine::new(client)
        .with_account_id("work".into())
        .with_options(EngineOptions {
            upload_chunk: 2 * UPLOAD_UNIT,
            ..EngineOptions::default()
        })
}

async fn demo() -> (MockGraphServer, GraphEngine) {
    let server = MockGraphServer::start_demo().await.unwrap();
    let engine = engine_for(&server).with_own_address(ME);
    (server, engine)
}

fn envelope(to: &[&str]) -> Envelope {
    Envelope {
        from: ME.into(),
        to: to.iter().map(|s| (*s).to_owned()).collect(),
    }
}

/// Deterministic incompressible-ish bytes.
fn bytes(len: usize, seed: u8) -> Vec<u8> {
    let mut x = u32::from(seed) | 1;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x & 0xFF) as u8
        })
        .collect()
}

fn b64_lines(data: &[u8]) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(data);
    encoded
        .as_bytes()
        .chunks(76)
        .map(|c| std::str::from_utf8(c).unwrap().to_owned() + "\r\n")
        .collect()
}

/// A message with a large and a small attachment (`big` bytes and 100 kB).
fn large_message(from: &str, sender: Option<&str>, big: usize, subject: &str) -> Vec<u8> {
    let sender = sender.map_or_else(String::new, |s| format!("Sender: {s}\r\n"));
    format!(
        "From: {from}\r\n{sender}To: Bob <bob@fabrikam.example>\r\nSubject: {subject}\r\nMessage-ID: <{subject}@contoso.example>\r\nIn-Reply-To: <root@contoso.example>\r\nReferences: <root@contoso.example>\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"b\"\r\n\r\n--b\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<p>The files.</p>\r\n--b\r\nContent-Type: application/pdf; name=\"big.pdf\"\r\nContent-Disposition: attachment; filename=\"big.pdf\"\r\nContent-Transfer-Encoding: base64\r\n\r\n{}--b\r\nContent-Type: image/png; name=\"small.png\"\r\nContent-Disposition: attachment; filename=\"small.png\"\r\nContent-Transfer-Encoding: base64\r\n\r\n{}--b--\r\n",
        b64_lines(&bytes(big, 7)),
        b64_lines(&bytes(100_000, 9)),
    )
    .into_bytes()
}

fn small_message(from: &str, sender: Option<&str>, subject: &str) -> Vec<u8> {
    let sender = sender.map_or_else(String::new, |s| format!("Sender: {s}\r\n"));
    format!(
        "From: {from}\r\n{sender}To: bob@fabrikam.example\r\nSubject: {subject}\r\nMessage-ID: <{subject}@contoso.example>\r\n\r\nHello.\r\n"
    )
    .into_bytes()
}

#[tokio::test]
async fn large_messages_go_up_in_upload_sessions_with_progress() {
    let (server, engine) = demo().await;
    let big = 5 * 1024 * 1024 + 12_345;
    let raw = large_message(ME, None, big, "large-1");
    assert!(raw.len() > 4 * 1024 * 1024);
    let reports: Mutex<Vec<(u64, u64)>> = Mutex::new(Vec::new());
    let progress = |sent: u64, total: u64| reports.lock().unwrap().push((sent, total));
    let receipt = engine
        .send_with_progress(
            &envelope(&["bob@fabrikam.example", "eve@fabrikam.example"]),
            &raw,
            &progress,
        )
        .await
        .unwrap();
    assert!(
        receipt.response.contains("2 attachments"),
        "{}",
        receipt.response
    );

    // Delivered through a draft, not sendMail.
    let sent = server.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].via, "send");
    assert_eq!(sent[0].mailbox, ME);
    let text = String::from_utf8_lossy(&sent[0].raw).into_owned();
    assert!(text.contains("Subject: large-1"));
    assert!(text.contains("Message-ID: <large-1@contoso.example>"));
    assert!(
        text.contains("In-Reply-To: <root@contoso.example>"),
        "threading survives"
    );
    assert!(
        text.contains("Bcc: <eve@fabrikam.example>"),
        "envelope-only recipients"
    );
    assert!(text.contains("filename=\"big.pdf\"") && text.contains("filename=\"small.png\""));
    // The big attachment arrived complete, in 640 KiB ranges.
    let uploads = server.completed_uploads();
    assert_eq!(uploads.len(), 1);
    assert_eq!(uploads[0].name, "big.pdf");
    assert_eq!(uploads[0].size, big);
    assert_eq!(uploads[0].ranges, big.div_ceil(2 * UPLOAD_UNIT));
    assert_eq!(server.open_uploads(), 0);
    let parsed = mail_parser::MessageParser::default()
        .parse(&sent[0].raw)
        .unwrap();
    let pdf = parsed
        .attachments()
        .find(|a| mail_parser::MimeHeaders::attachment_name(*a) == Some("big.pdf"))
        .unwrap();
    assert_eq!(pdf.contents(), bytes(big, 7).as_slice());

    // Progress: monotonic, ends at the total.
    let reports = reports.into_inner().unwrap();
    assert!(reports.len() >= uploads[0].ranges + 2, "{}", reports.len());
    assert!(
        reports
            .windows(2)
            .all(|w| w[0].0 <= w[1].0 && w[0].1 == w[1].1)
    );
    let (last, total) = *reports.last().unwrap();
    assert_eq!(last, total);
    assert!(total >= (big + 100_000) as u64);

    // No request carried more than 4 MB, no MIME sendMail, the token never
    // went to the upload URL (the mock refuses it), drafts are gone, the
    // copy is in Sent Items.
    let requests = server.requests();
    assert!(requests.iter().all(|r| r.body.len() <= 4 * 1024 * 1024));
    assert!(!requests.iter().any(|r| r.path.ends_with("/sendMail")));
    assert!(
        requests
            .iter()
            .filter(|r| r.path.starts_with("/upload/"))
            .all(|r| r.token.is_none())
    );
    assert!(
        server
            .messages_in(None, "drafts")
            .iter()
            .all(|m| !String::from_utf8_lossy(&m.raw).contains("large-1"))
    );
    assert!(
        server
            .messages_in(None, "sentitems")
            .iter()
            .any(|m| String::from_utf8_lossy(&m.raw).contains("Subject: large-1"))
    );

    // Small messages still go as MIME through sendMail, without progress.
    let reports: Mutex<Vec<(u64, u64)>> = Mutex::new(Vec::new());
    let progress = |sent: u64, total: u64| reports.lock().unwrap().push((sent, total));
    engine
        .send_with_progress(
            &envelope(&["bob@fabrikam.example"]),
            &small_message(ME, None, "small-1"),
            &progress,
        )
        .await
        .unwrap();
    assert_eq!(server.sent()[1].via, "sendMail");
    assert!(reports.into_inner().unwrap().is_empty());
}

#[tokio::test]
async fn upload_ranges_are_retried_and_failures_clean_up() {
    let (server, engine) = demo().await;
    let big = 4 * 1024 * 1024;
    // A transient failure of one range is repeated.
    server.inject(
        InjectedError::unavailable(Some(0))
            .on("/upload/")
            .method("PUT"),
    );
    engine
        .send(
            &envelope(&["bob@fabrikam.example"]),
            &large_message(ME, None, big, "retry-1"),
        )
        .await
        .unwrap();
    assert_eq!(server.sent().len(), 1);
    assert_eq!(server.completed_uploads()[0].size, big);

    // A refused range fails the send: the draft is deleted, nothing sent.
    let drafts_before = server.messages_in(None, "drafts").len();
    server.inject(
        InjectedError::status(400)
            .on("/upload/")
            .method("PUT")
            .times(10),
    );
    let err = engine
        .send(
            &envelope(&["bob@fabrikam.example"]),
            &large_message(ME, None, big, "fail-1"),
        )
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Rejected);
    assert_eq!(server.sent().len(), 1);
    let drafts: Vec<_> = server
        .messages_in(None, "drafts")
        .into_iter()
        .filter(|m| String::from_utf8_lossy(&m.raw).contains("fail-1"))
        .collect();
    assert!(drafts.is_empty(), "the unsent draft is deleted");
    assert!(server.messages_in(None, "drafts").len() <= drafts_before + 1);

    // A signed message over 4 MB cannot be rebuilt: refused before any
    // request.
    let mut signed = format!("From: {ME}\r\nTo: bob@fabrikam.example\r\nContent-Type: multipart/signed; protocol=\"application/pkcs7-signature\"; boundary=s\r\n\r\n--s\r\n").into_bytes();
    signed.extend(vec![b'x'; 5 * 1024 * 1024]);
    server.clear_requests();
    let err = engine
        .send(&envelope(&["bob@fabrikam.example"]), &signed)
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::TooLarge);
    assert!(
        server.requests().iter().all(|r| r.method == "GET"),
        "no draft created"
    );

    // The mock enforces Graph's 4 MB request limit: a MIME upload of a
    // large message would be refused.
    let large = large_message(ME, None, big, "mime-1");
    assert!(nail_proto_graph::upload::mime_upload(&large).is_none());
}

#[tokio::test]
async fn send_as_and_on_behalf_pick_the_mailbox() {
    let (server, engine) = demo().await;
    let to = envelope(&["bob@fabrikam.example"]);
    // Own address (any case): the user's mailbox.
    engine
        .send(
            &to,
            &small_message("Ada <ADA@contoso.example>", None, "own-1"),
        )
        .await
        .unwrap();
    // From the shared mailbox: send as, the copy in its Sent Items.
    engine
        .send(
            &to,
            &small_message(&format!("Support <{SHARED}>"), None, "as-1"),
        )
        .await
        .unwrap();
    // From the shared mailbox with the user as Sender: on behalf, from the
    // user's own mailbox.
    engine
        .send(
            &to,
            &small_message(SHARED, Some(&format!("Ada <{ME}>")), "behalf-1"),
        )
        .await
        .unwrap();
    let sent = server.sent();
    let mailboxes: Vec<&str> = sent.iter().map(|s| s.mailbox.as_str()).collect();
    assert_eq!(mailboxes, [ME, SHARED, ME]);
    assert!(String::from_utf8_lossy(&sent[2].raw).contains(&format!("Sender: Ada <{ME}>")));
    let shared_copy = server
        .messages_in(Some(SHARED), "sentitems")
        .into_iter()
        .any(|m| String::from_utf8_lossy(&m.raw).contains("as-1"));
    assert!(shared_copy);

    // Large mail as the shared mailbox: the draft is created there.
    engine
        .send(
            &to,
            &large_message(SHARED, None, 4 * 1024 * 1024, "as-large"),
        )
        .await
        .unwrap();
    let last = server.sent().pop().unwrap();
    assert_eq!((last.mailbox.as_str(), last.via.as_str()), (SHARED, "send"));
    // Large on-behalf mail: from the user's mailbox with from + sender.
    engine
        .send(
            &to,
            &large_message(SHARED, Some(ME), 4 * 1024 * 1024, "behalf-large"),
        )
        .await
        .unwrap();
    let last = server.sent().pop().unwrap();
    assert_eq!(last.mailbox, ME);
    let text = String::from_utf8_lossy(&last.raw).into_owned();
    assert!(
        text.contains(&format!("From: <{SHARED}>")) && text.contains(&format!("Sender: <{ME}>"))
    );

    // A mailbox the user may not send as: forbidden.
    let err = engine
        .send(&to, &small_message("ceo@contoso.example", None, "denied-1"))
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Forbidden);
}

#[tokio::test]
async fn the_own_address_is_looked_up_once_when_not_configured() {
    let server = MockGraphServer::start_demo().await.unwrap();
    let engine = engine_for(&server);
    assert_eq!(engine.own_address().await.unwrap(), ME);
    assert_eq!(engine.own_address().await.unwrap(), ME);
    let lookups = server
        .requests()
        .iter()
        .filter(|r| r.path == "/v1.0/me")
        .count();
    assert_eq!(lookups, 1);
    // Configured: no lookup at all.
    let configured = engine_for(&server).with_own_address(" Ada@Contoso.example ");
    server.clear_requests();
    assert_eq!(configured.own_address().await.unwrap(), ME);
    assert!(server.requests().is_empty());
}

#[tokio::test]
async fn large_appends_are_created_from_parts_and_not_drafts() {
    let (server, engine) = demo().await;
    let raw = large_message(
        "bob@fabrikam.example",
        None,
        4 * 1024 * 1024 + 1,
        "append-1",
    );
    let flags = Flags {
        seen: true,
        flagged: true,
        ..Flags::default()
    };
    let outcome = ExchangeMailEngine::write(
        &engine,
        None,
        &RemoteWrite::Append {
            folder: "inbox".into(),
            raw: raw.clone(),
            flags,
        },
    )
    .await
    .unwrap();
    let WriteOutcome::Done {
        remote_id: Some(id),
    } = outcome
    else {
        panic!("{outcome:?}");
    };
    let stored = server.message(None, &id).unwrap();
    assert_eq!(stored.folder, server.folder_id(None, "inbox"));
    assert!(!stored.is_draft, "not shown as unsent");
    assert!(stored.is_read);
    assert_eq!(stored.flag, "flagged");
    assert!(String::from_utf8_lossy(&stored.raw).contains("filename=\"big.pdf\""));
    assert_eq!(server.completed_uploads().len(), 1);
    // Replaying the append finds the copy by its Message-ID.
    let again = ExchangeMailEngine::write(
        &engine,
        None,
        &RemoteWrite::Append {
            folder: "inbox".into(),
            raw,
            flags,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        again,
        WriteOutcome::Done {
            remote_id: Some(id)
        }
    );
    assert_eq!(server.completed_uploads().len(), 1);
}

#[tokio::test]
async fn headers_only_fetches() {
    let (server, engine) = demo().await;
    let id = server.deliver(
        None,
        "inbox",
        b"From: bob@fabrikam.example\r\nTo: ada@contoso.example\r\nSubject: Weekly\r\n news\r\nMessage-ID: <w1@x>\r\n\r\nA long body nobody downloads.\r\n",
    );
    let headers = engine.fetch_headers(None, &id).await.unwrap();
    let text = String::from_utf8(headers).unwrap();
    assert!(text.contains("Subject: Weekly news\r\n"), "{text}");
    assert!(text.contains("Message-ID: <w1@x>\r\n"));
    assert!(text.ends_with("\r\n\r\n"));
    assert!(!text.contains("nobody downloads"));
    let req = server
        .requests()
        .into_iter()
        .rev()
        .find(|r| r.path.contains(&id.replace('=', "%3D")) || r.path.contains(&id))
        .unwrap();
    assert!(req.query.contains("internetMessageHeaders"));

    // Sent mail has no transport headers: the MIME is cut at the headers.
    let sent = server.deliver(
        None,
        "sentitems",
        b"From: ada@contoso.example\r\nSubject: Mine\r\n\r\nBody text.\r\n",
    );
    let text = String::from_utf8(engine.fetch_headers(None, &sent).await.unwrap()).unwrap();
    assert_eq!(text, "From: ada@contoso.example\r\nSubject: Mine\r\n\r\n");

    // A gone message is not found.
    let err = ExchangeMailEngine::fetch_mime(&engine, None, "AAMk-missing=")
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
    assert!(engine.fetch_headers(None, "AAMk-missing=").await.is_err());
}

#[tokio::test]
async fn moves_between_mailboxes() {
    let (server, engine) = demo().await;
    let id = server.deliver(
        None,
        "inbox",
        &small_message("bob@fabrikam.example", None, "move-1"),
    );
    // Within one mailbox: Graph's own move.
    let archived = engine
        .move_to_mailbox(None, &id, Some(ME), "archive", Flags::default())
        .await
        .unwrap();
    assert!(server.requests().iter().any(|r| r.path.ends_with("/move")));
    assert_eq!(
        server.message(None, &archived).unwrap().folder,
        server.folder_id(None, "archive")
    );
    // To the shared mailbox: copied there, deleted here.
    let flags = Flags {
        seen: true,
        ..Flags::default()
    };
    let moved = engine
        .move_to_mailbox(None, &archived, Some(SHARED), "inbox", flags)
        .await
        .unwrap();
    assert!(server.message(None, &archived).is_none());
    let copy = server.message(Some(SHARED), &moved).unwrap();
    assert!(String::from_utf8_lossy(&copy.raw).contains("Subject: move-1"));
    assert!(copy.is_read);
    assert_eq!(copy.folder, server.folder_id(Some(SHARED), "inbox"));
    // And back, large: through parts.
    let big = server.deliver(
        Some(SHARED),
        "inbox",
        &large_message("bob@fabrikam.example", None, 4 * 1024 * 1024, "move-large"),
    );
    let back = engine
        .move_to_mailbox(Some(SHARED), &big, None, "inbox", Flags::default())
        .await
        .unwrap();
    assert!(server.message(Some(SHARED), &big).is_none());
    assert!(String::from_utf8_lossy(&server.message(None, &back).unwrap().raw).contains("big.pdf"));
    // A mailbox without access: nothing is deleted.
    let keep = server.deliver(
        None,
        "inbox",
        &small_message("bob@fabrikam.example", None, "keep-1"),
    );
    assert!(
        engine
            .move_to_mailbox(
                None,
                &keep,
                Some("ceo@contoso.example"),
                "inbox",
                Flags::default()
            )
            .await
            .is_err()
    );
    assert!(server.message(None, &keep).is_some());
}

#[tokio::test]
async fn conversation_ids_are_thread_ids() {
    let (_server, engine) = demo().await;
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
        assert!(conversation.is_some());
        assert_eq!(
            m.thread_id, conversation,
            "the conversation id is the thread id"
        );
    }
}

#[test]
fn draft_parts_of_a_generated_large_message() {
    let raw = large_message(ME, Some(SHARED), 3 * 1024 * 1024 + 1, "parts-1");
    let parts = DraftParts::from_mime(&raw).unwrap();
    assert_eq!(parts.attachments.len(), 2);
    assert_eq!(parts.attachments[0].bytes.len(), 3 * 1024 * 1024 + 1);
    assert_eq!(parts.message["sender"]["emailAddress"]["address"], SHARED);
}
