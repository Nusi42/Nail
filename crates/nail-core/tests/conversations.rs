//! Conversations in the core (P2 WP201, ADR 0148): list rows count the
//! user's replies from Sent, "me" comes from the accounts' addresses, the
//! conversation request reads oldest first, and remote sessions see only
//! what their grant covers (the rows are rebuilt, nothing leaks).

#![cfg(test)]

use nail_api::remote::RemoteAccessPreset;
use nail_api::remote::{AuthRequest, AuthResponse, DEVICE_COOKIE, RemoteGrant, SESSION_COOKIE};
use nail_api::{
    AccountId, ConversationMessage, ErrorCode, Folder, FolderId, FolderRole, Request, Response,
    ThreadSummary,
};
use nail_core::demo::{DEMO_ACCOUNT_ID, demo_options, seed_demo};
use nail_core::{Service, ServiceOptions};
use nail_remote_auth::{ClientMeta, Cookies, RemoteSessionCtx};
use pretty_assertions::assert_eq;

const ME: &str = "sam@nail.example";
const SUBJECT: &str = "Project Nightingale kickoff";

struct Fixture {
    service: Service,
    _data: tempfile::TempDir,
    _config: tempfile::TempDir,
}

fn setup() -> Fixture {
    setup_with(None)
}

/// The demo data, optionally with an `accounts.kdl`.
fn setup_with(accounts: Option<&str>) -> Fixture {
    let data = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    if let Some(accounts) = accounts {
        std::fs::write(config.path().join("accounts.kdl"), accounts).unwrap();
    }
    let mut options = ServiceOptions::new(data.path(), config.path());
    options.watch_config = false;
    options.start_engines = false;
    let service = Service::open(demo_options(options)).unwrap();
    seed_demo(service.store(), service.blobs()).unwrap();
    Fixture {
        service,
        _data: data,
        _config: config,
    }
}

fn folder(f: &Fixture, role: FolderRole) -> Folder {
    f.service
        .store()
        .list_folders(&AccountId::new(DEMO_ACCOUNT_ID))
        .unwrap()
        .into_iter()
        .find(|folder| folder.role == role)
        .unwrap()
}

fn threads(response: Response) -> (u32, Vec<ThreadSummary>) {
    match response {
        Response::Threads { total, threads, .. } => (total, threads),
        other => panic!("{other:?}"),
    }
}

fn conversation(response: Response) -> (ThreadSummary, Vec<ConversationMessage>) {
    match response {
        Response::Conversation { thread, messages } => (*thread, messages),
        other => panic!("{other:?}"),
    }
}

fn list(folder: &FolderId) -> Request {
    Request::ListThreads {
        folder_id: folder.clone(),
        offset: 0,
        limit: 100,
    }
}

fn nightingale(rows: &[ThreadSummary]) -> &ThreadSummary {
    rows.iter().find(|t| t.subject == SUBJECT).unwrap()
}

fn from_me(messages: &[ConversationMessage]) -> Vec<bool> {
    messages.iter().map(|m| m.from_me).collect()
}

#[tokio::test]
async fn an_inbox_row_is_the_whole_conversation_with_the_users_replies() {
    let f = setup();
    let inbox = folder(&f, FolderRole::Inbox);
    let sent = folder(&f, FolderRole::Sent);
    let (_, rows) = threads(f.service.handle(list(&inbox.id)).await);
    let row = nightingale(&rows);
    assert_eq!(
        row.message_count, 7,
        "four in the Inbox, three replies from Sent"
    );
    let me: Vec<(&str, bool)> = row
        .participants
        .iter()
        .map(|p| (p.address.email.as_str(), p.me))
        .collect();
    assert_eq!(
        me,
        [
            ("grace@navy.example", false),
            (ME, true),
            ("ada@lovelace.example", false)
        ]
    );
    assert_eq!(row.latest_from.as_ref().map(|p| p.me), Some(false));
    assert!(row.date.starts_with("2026-09-22"), "{}", row.date);

    let (thread, messages) = conversation(
        f.service
            .handle(Request::GetConversation {
                thread_id: row.id.clone(),
                folder_id: Some(inbox.id.clone()),
            })
            .await,
    );
    assert_eq!(&thread, row, "the reader and the row agree");
    assert_eq!(
        from_me(&messages),
        [false, true, false, true, false, true, false]
    );
    let dates: Vec<&str> = messages.iter().map(|m| m.message.date.as_str()).collect();
    let mut sorted = dates.clone();
    sorted.sort_unstable();
    assert_eq!(dates, sorted, "oldest first");
    assert!(
        messages
            .iter()
            .filter(|m| m.from_me)
            .all(|m| m.message.folder_id == sent.id)
    );

    // Without a folder (searching): the same seven, across folders.
    let (_, everywhere) = conversation(
        f.service
            .handle(Request::GetConversation {
                thread_id: row.id.clone(),
                folder_id: None,
            })
            .await,
    );
    assert_eq!(everywhere.len(), 7);

    // GetThread resolves "me" too.
    match f
        .service
        .handle(Request::GetThread {
            thread_id: row.id.clone(),
        })
        .await
    {
        Response::Thread { thread, .. } => {
            assert!(
                thread
                    .participants
                    .iter()
                    .any(|p| p.address.email == ME && p.me)
            );
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn the_unified_inbox_counts_the_same_and_unknown_ids_fail() {
    let f = setup();
    let (_, rows) = threads(f.service.handle(list(&FolderId::new("all/inbox"))).await);
    assert_eq!(nightingale(&rows).message_count, 7);
    let unknown = f
        .service
        .handle(Request::GetConversation {
            thread_id: "t-unknown".into(),
            folder_id: None,
        })
        .await;
    assert!(matches!(unknown, Response::Error { .. }), "{unknown:?}");
    let unknown_folder = f
        .service
        .handle(list(&FolderId::new("no-such-folder")))
        .await;
    assert!(
        matches!(unknown_folder, Response::Error { .. }),
        "{unknown_folder:?}"
    );
}

/// A user with `grant` and a browser paired to them: the remote session.
async fn remote_session(f: &Fixture, name: &str, grant: RemoteGrant) -> RemoteSessionCtx {
    let user = match f
        .service
        .handle(Request::CreateRemoteUser {
            name: name.into(),
            display_name: None,
            grant,
        })
        .await
    {
        Response::RemoteUserSaved { user } => user,
        other => panic!("{other:?}"),
    };
    let code = match f
        .service
        .handle(Request::CreatePairingCode {
            user_id: user.id.clone(),
            device_name: Some("Phone".into()),
            grant: None,
            ttl_minutes: None,
        })
        .await
    {
        Response::PairingCodeCreated { code, .. } => code,
        other => panic!("{other:?}"),
    };
    let auth = f.service.remote_auth();
    let client = ClientMeta {
        ip: Some("192.168.1.20".parse().unwrap()),
        user_agent: Some("Test".into()),
    };
    let anonymous = Cookies::default();
    let csrf = auth.csrf_token(&anonymous);
    let reply = auth
        .handle_auth(
            AuthRequest::Pair {
                code,
                device_name: "Phone".into(),
            },
            &anonymous,
            &client,
            Some(&csrf),
        )
        .await;
    assert!(
        matches!(reply.response, AuthResponse::LoggedIn { .. }),
        "{:?}",
        reply.response
    );
    let mut cookies = Cookies::default();
    for set in &reply.cookies {
        let (pair, _) = set.split_once(';').unwrap_or((set, ""));
        let (name, value) = pair.split_once('=').unwrap();
        let value = Some(zeroize::Zeroizing::new(value.to_owned()));
        match name {
            SESSION_COOKIE => cookies.session = value,
            DEVICE_COOKIE => cookies.device = value,
            other => panic!("{other}"),
        }
    }
    auth.resolve(&cookies, &client).unwrap().ctx
}

#[tokio::test]
async fn a_grant_without_sent_sees_neither_the_replies_nor_traces_of_them() {
    let f = setup();
    let inbox = folder(&f, FolderRole::Inbox);
    let sent = folder(&f, FolderRole::Sent);
    let mut grant = RemoteAccessPreset::ReadOnly.grant();
    grant.mail.excluded_folders = vec![sent.id.clone()];
    let ctx = remote_session(&f, "no-sent", grant).await;
    let service = &f.service;

    let (_, rows) = threads(service.handle_remote(&ctx, list(&inbox.id)).await);
    let row = nightingale(&rows);
    assert_eq!(row.message_count, 4, "the Sent replies are not granted");
    assert!(row.participants.iter().all(|p| p.address.email != ME));
    assert_ne!(row.latest_message.folder_id, sent.id);

    // The conversation for the listed folder, and without a folder: the
    // replies are dropped and the row is rebuilt from what is left.
    for folder_id in [Some(inbox.id.clone()), None] {
        let (thread, messages) = conversation(
            service
                .handle_remote(
                    &ctx,
                    Request::GetConversation {
                        thread_id: row.id.clone(),
                        folder_id,
                    },
                )
                .await,
        );
        assert_eq!(messages.len(), 4);
        assert!(messages.iter().all(|m| m.message.folder_id != sent.id));
        assert_eq!(thread.message_count, 4);
        assert!(thread.participants.iter().all(|p| p.address.email != ME));
    }
    // GetThread too: its row no longer counts or names the replies.
    match service
        .handle_remote(
            &ctx,
            Request::GetThread {
                thread_id: row.id.clone(),
            },
        )
        .await
    {
        Response::Thread { thread, messages } => {
            assert_eq!(messages.len(), 4);
            assert_eq!(thread.message_count, 4);
            assert!(thread.participants.iter().all(|p| p.address.email != ME));
        }
        other => panic!("{other:?}"),
    }

    // The Sent folder itself is refused; a conversation only in Sent is
    // forbidden, not empty.
    let refused = service.handle_remote(&ctx, list(&sent.id)).await;
    assert!(
        matches!(
            &refused,
            Response::Error {
                code: ErrorCode::Forbidden,
                ..
            }
        ),
        "{refused:?}"
    );
    let dinner = f
        .service
        .store()
        .list_messages(&sent.id, 0, 100)
        .unwrap()
        .messages
        .into_iter()
        .find(|m| m.subject.contains("inner"))
        .and_then(|m| m.thread_id)
        .unwrap();
    let hidden = service
        .handle_remote(
            &ctx,
            Request::GetConversation {
                thread_id: dinner,
                folder_id: None,
            },
        )
        .await;
    assert!(
        matches!(
            &hidden,
            Response::Error {
                code: ErrorCode::Forbidden,
                ..
            }
        ),
        "{hidden:?}"
    );

    // A full grant sees everything.
    let full = remote_session(&f, "full", RemoteAccessPreset::ReadOnly.grant()).await;
    let (_, rows) = threads(service.handle_remote(&full, list(&inbox.id)).await);
    assert_eq!(nightingale(&rows).message_count, 7);
}

#[tokio::test]
async fn identities_from_accounts_kdl_are_me_case_insensitively() {
    // The demo account with an extra identity: Grace's address (written in
    // capitals) becomes one of the user's.
    let f = setup_with(Some(&format!(
        "account \"{DEMO_ACCOUNT_ID}\" email=\"{ME}\" name=\"Demo\" {{
             imap host=\"127.0.0.1\" port=1 tls=\"plaintext\"
             smtp host=\"127.0.0.1\" port=1 tls=\"plaintext\"
             identity \"{ME}\" name=\"Sam\"
             identity \"GRACE@navy.example\" name=\"Grace\"
}}
"
    )));
    let inbox = folder(&f, FolderRole::Inbox);
    let (_, rows) = threads(f.service.handle(list(&inbox.id)).await);
    let row = nightingale(&rows);
    let grace = row
        .participants
        .iter()
        .find(|p| p.address.email == "grace@navy.example")
        .unwrap();
    assert!(grace.me);
    assert_eq!(
        row.latest_from.as_ref().map(|p| p.me),
        Some(true),
        "Grace wrote last"
    );
    let (_, messages) = conversation(
        f.service
            .handle(Request::GetConversation {
                thread_id: row.id.clone(),
                folder_id: Some(inbox.id.clone()),
            })
            .await,
    );
    let ada = messages
        .iter()
        .find(|m| {
            m.message
                .from
                .as_ref()
                .is_some_and(|a| a.email.starts_with("ada@"))
        })
        .unwrap();
    assert!(!ada.from_me);
    assert_eq!(messages.iter().filter(|m| m.from_me).count(), 6);
}

#[tokio::test]
async fn a_send_as_identity_is_a_shared_mailbox_not_me() {
    // Grace's address as a shared mailbox the user sends as (ADR 0094):
    // colleagues write from it too, so her mail stays someone else's, and
    // the row acts on her newest message, not on an older one (P2 review).
    let f = setup_with(Some(&format!(
        "account \"{DEMO_ACCOUNT_ID}\" email=\"{ME}\" name=\"Demo\" {{
             imap host=\"127.0.0.1\" port=1 tls=\"plaintext\"
             smtp host=\"127.0.0.1\" port=1 tls=\"plaintext\"
             identity \"{ME}\" name=\"Sam\"
             identity \"GRACE@navy.example\" name=\"Navy\" send-as=\"send-as\"
             identity \"ada@analytical.example\" name=\"Ada's desk\" send-as=\"on-behalf\" sender=\"{ME}\"
}}
"
    )));
    let inbox = folder(&f, FolderRole::Inbox);
    let (_, rows) = threads(f.service.handle(list(&inbox.id)).await);
    let row = nightingale(&rows);
    let grace = row
        .participants
        .iter()
        .find(|p| p.address.email == "grace@navy.example")
        .unwrap();
    assert!(!grace.me);
    let latest = row.latest_from.as_ref().unwrap();
    assert_eq!(latest.address.email, "grace@navy.example");
    assert!(!latest.me);
    assert_eq!(
        row.latest_message.from.as_ref().map(|a| a.email.as_str()),
        Some("grace@navy.example"),
        "the row acts on Grace's newest message"
    );
    let (_, messages) = conversation(
        f.service
            .handle(Request::GetConversation {
                thread_id: row.id.clone(),
                folder_id: Some(inbox.id.clone()),
            })
            .await,
    );
    // Only the replies from Sent are the user's; Ada's on-behalf mailbox
    // and Grace's shared one are not.
    let mine: Vec<_> = messages.iter().filter(|m| m.from_me).collect();
    assert!(!mine.is_empty());
    assert!(mine.iter().all(|m| {
        m.message
            .from
            .as_ref()
            .is_some_and(|a| a.email.eq_ignore_ascii_case(ME))
    }));
}
