//! WP125 core wiring of the WP123 leftovers (ADRs 0097, 0099) against the
//! mock Graph server: folder sync policies for Exchange (headers only,
//! bodies on demand), Graph conversation ids as Nail threads, moves into a
//! shared mailbox and into another Exchange account.

#![cfg(test)]

#[expect(dead_code, reason = "each test crate uses part of the shared helpers")]
mod common;
use common::{TIMEOUT, api_service, eventually, folders, messages, mock_oauth, oauth_provider, ok};
use nail_core::testing::TempService;

use nail_api::accounts::SecretKind;
use nail_api::exchange::ExchangeMailboxSettings;
use nail_api::{AccountId, FolderRole, MessageSummary, MoveTarget, Request, Response};
use nail_core::Service;
use nail_proto_graph::testing::MockGraphServer;
use nail_testkit_net::MockOAuthServer;

struct Env {
    service: Service,
    graph: MockGraphServer,
    other: MockGraphServer,
    _oauth: MockOAuthServer,
    _temp: TempService,
}

/// Two Graph accounts (`contoso`, and `fabrikam` on a second mock
/// tenant); `sync` goes into the first account's node.
async fn setup(sync: &str) -> Env {
    let graph = MockGraphServer::start_demo().await.unwrap();
    let other = MockGraphServer::start("bo@fabrikam.example").await.unwrap();
    let oauth = mock_oauth(&[
        ("refresh-ex", "ada@contoso.example", "graph"),
        ("refresh-fab", "bo@fabrikam.example", "graph"),
    ])
    .await;
    let temp = api_service(
        &format!(
            r#"{provider}account "contoso" email="ada@contoso.example" name="Contoso" {{
    exchange host="127.0.0.1" port={port} tls="plaintext" poll-interval=10 max-poll-interval=10 {{
        auth "oauth2" provider="mockms"
    }}
    identity "ada@contoso.example" name="Ada Lovelace"
    {sync}
}}
account "fabrikam" email="bo@fabrikam.example" name="Fabrikam" {{
    exchange host="127.0.0.1" port={other_port} tls="plaintext" poll-interval=10 max-poll-interval=10 {{
        auth "oauth2" provider="mockms"
    }}
    identity "bo@fabrikam.example" name="Bo"
}}
"#,
            provider = oauth_provider("mockms", &oauth, "graph"),
            port = graph.addr().port(),
            other_port = other.addr().port(),
        ),
        &[
            ("contoso", SecretKind::OAuth2, "refresh-ex"),
            ("fabrikam", SecretKind::OAuth2, "refresh-fab"),
        ],
    );
    Env {
        service: temp.service.clone(),
        graph,
        other,
        _oauth: oauth,
        _temp: temp,
    }
}

async fn folder_messages(service: &Service, folder: &nail_api::Folder) -> Vec<MessageSummary> {
    messages(service, &folder.id).await
}

async fn inbox_folder(service: &Service, account: &str) -> Option<nail_api::Folder> {
    folders(service, account)
        .await
        .into_iter()
        .find(|f| f.role == FolderRole::Inbox && !f.path.starts_with('@'))
}

async fn inbox(service: &Service, account: &str) -> Vec<MessageSummary> {
    match inbox_folder(service, account).await {
        Some(folder) => folder_messages(service, &folder).await,
        None => Vec::new(),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn exchange_folders_follow_their_sync_policy_and_fetch_bodies_on_demand() {
    let env = setup(r#"sync bodies="headers-only""#).await;
    let service = &env.service;
    eventually("the Graph inbox", || async {
        inbox(service, "contoso").await.len() == 2
    })
    .await;
    let list = inbox(service, "contoso").await;
    assert!(list.iter().all(|m| m.headers_only), "{list:?}");
    // Opening one downloads its body.
    let id = list[0].id.clone();
    let Response::Message { message } = ok(service, Request::GetMessage { id: id.clone() }).await
    else {
        panic!("message expected");
    };
    assert!(!message.text.is_empty() || message.sanitized_html.is_some());
    eventually("the body is stored", || async {
        inbox(service, "contoso")
            .await
            .iter()
            .any(|m| m.id == id && !m.headers_only)
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn graph_conversations_are_nail_threads() {
    let env = setup("").await;
    let service = &env.service;
    eventually("the Graph inbox", || async {
        inbox(service, "contoso").await.len() == 2
    })
    .await;
    // Two originals with the same subject and no references: only the
    // server's conversation id groups them.
    for (n, from) in ["alan@example.org", "grace@example.org"].iter().enumerate() {
        let raw = format!(
            "From: {from}\r\nTo: ada@contoso.example\r\nSubject: Quarterly numbers\r\nMessage-ID: <q{n}@example.org>\r\nDate: Thu, 01 Oct 2026 0{n}:00:00 +0000\r\n\r\nNumbers {n}.\r\n"
        );
        env.graph.deliver(None, "inbox", raw.as_bytes());
    }
    ok(
        service,
        Request::SyncNow {
            account_id: Some(AccountId::new("contoso")),
        },
    )
    .await;
    eventually("both delivered messages", || async {
        inbox(service, "contoso")
            .await
            .iter()
            .filter(|m| m.subject == "Quarterly numbers")
            .count()
            == 2
    })
    .await;
    let list = inbox(service, "contoso").await;
    let threads: Vec<_> = list
        .iter()
        .filter(|m| m.subject == "Quarterly numbers")
        .map(|m| m.thread_id.clone())
        .collect();
    assert!(threads[0].is_some());
    assert_eq!(threads[0], threads[1], "one thread by conversation id");
}

#[tokio::test(flavor = "multi_thread")]
async fn messages_move_into_a_shared_mailbox_and_into_another_account() {
    let env = setup("").await;
    let service = &env.service;
    eventually("both inboxes", || async {
        inbox(service, "contoso").await.len() == 2
            && inbox_folder(service, "fabrikam").await.is_some()
    })
    .await;
    ok(
        service,
        Request::AddExchangeMailbox {
            account_id: AccountId::new("contoso"),
            settings: ExchangeMailboxSettings::shared("support@contoso.example"),
        },
    )
    .await;
    ok(
        service,
        Request::SyncNow {
            account_id: Some(AccountId::new("contoso")),
        },
    )
    .await;
    let shared_inbox = || async {
        folders(service, "contoso")
            .await
            .into_iter()
            .find(|f| f.path == "@support@contoso.example/Inbox")
    };
    eventually("the shared mailbox", || async {
        shared_inbox().await.is_some()
    })
    .await;
    let shared = shared_inbox().await.unwrap();
    let list = inbox(service, "contoso").await;
    let first = list[0].clone();
    let second = list[1].clone();

    // Into the shared mailbox of the same account.
    ok(
        service,
        Request::MoveMessages {
            message_ids: vec![first.id.clone()],
            target: MoveTarget::FolderId(shared.id.clone()),
        },
    )
    .await;
    let shared_remote = env
        .graph
        .folder_id(Some("support@contoso.example"), "inbox");
    eventually("the move into the shared mailbox", || async {
        env.graph
            .messages_in(Some("support@contoso.example"), &shared_remote)
            .iter()
            .any(|m| String::from_utf8_lossy(&m.raw).contains(&first.subject))
    })
    .await;

    // Into another Exchange account.
    let target = inbox_folder(service, "fabrikam").await.unwrap();
    ok(
        service,
        Request::MoveMessages {
            message_ids: vec![second.id.clone()],
            target: MoveTarget::FolderId(target.id.clone()),
        },
    )
    .await;
    let other_inbox = env.other.folder_id(None, "inbox");
    eventually("the move into the other account", || async {
        env.other
            .messages_in(None, &other_inbox)
            .iter()
            .any(|m| String::from_utf8_lossy(&m.raw).contains(&second.subject))
    })
    .await;
    let own_inbox = env.graph.folder_id(None, "inbox");
    eventually("gone from the source", || async {
        !env.graph
            .messages_in(None, &own_inbox)
            .iter()
            .any(|m| String::from_utf8_lossy(&m.raw).contains(&second.subject))
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_large_message_goes_up_in_pieces_with_send_progress() {
    use base64::Engine as _;
    let env = setup("").await;
    let service = &env.service;
    eventually("the Graph inbox", || async {
        inbox(service, "contoso").await.len() == 2
    })
    .await;
    // 5 MiB of incompressible-looking bytes: over Graph's 4 MB limit.
    let bytes: Vec<u8> = (0..5 * 1024 * 1024u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 13).to_le_bytes()[0])
        .collect();
    let Response::AttachmentUploaded { attachment } = ok(
        service,
        Request::UploadAttachment {
            filename: "scan.bin".into(),
            mime: "application/octet-stream".into(),
            data: base64::engine::general_purpose::STANDARD.encode(&bytes),
            content_id: None,
        },
    )
    .await
    else {
        panic!("attachment expected");
    };
    let mut draft = nail_api::Draft::new("ada@contoso.example");
    draft.to = vec![nail_api::Address {
        name: None,
        email: "bob@contoso.example".into(),
    }];
    draft.subject = "The scan".into();
    draft.text = "Attached.".into();
    draft.attachments = vec![attachment];
    let Response::DraftSaved { draft_id } = ok(service, Request::SaveDraft { draft }).await else {
        panic!("draft expected");
    };
    let mut events = service.subscribe();
    ok(
        service,
        Request::SendDraft {
            draft_id,
            delay_secs: Some(0),
            send_at: None,
            crypto: None,
            expected_revision: None,
        },
    )
    .await;
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    let mut progress = Vec::new();
    while env.graph.completed_uploads().is_empty() || progress.is_empty() {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(left, events.recv()).await {
            Ok(Ok(nail_api::Event::SendProgress {
                sent_bytes,
                total_bytes,
                ..
            })) => progress.push((sent_bytes, total_bytes)),
            Ok(Ok(_) | Err(_)) => {}
            Err(elapsed) => panic!("no upload progress ({elapsed}); saw {progress:?}"),
        }
    }
    let (_, total) = progress[0];
    assert!(total > 4 * 1024 * 1024, "{progress:?}");
    assert!(progress.windows(2).all(|w| w[0].0 <= w[1].0));
    eventually("sent", || async {
        env.graph
            .sent()
            .iter()
            .any(|s| String::from_utf8_lossy(&s.raw).contains("The scan"))
            || env
                .graph
                .completed_uploads()
                .iter()
                .any(|u| u.name == "scan.bin")
    })
    .await;
}
