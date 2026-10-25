//! The M1a core end to end against the `demo-multi` mock servers: account
//! engines (IMAP with IDLE, POP3), the operation queue replayed to the
//! servers, offline replay, sending with undo, virtual accounts, search,
//! threads and account setup.

#![cfg(test)]

#[expect(dead_code, reason = "each test crate uses part of the shared helpers")]
mod common;
use common::{
    Multi, TIMEOUT, eventually, find, folder, folders, inbox_count, messages, next_matching,
    op_ids, request, save, statuses,
};
use nail_core::testing::error_of;

use std::time::Duration;

use nail_api::accounts::{
    AccountConfig, ComposerMode, EndpointSlot, IncomingServer, MailProtocol, SecretInput,
    SecretValue, VirtualAccountConfig,
};
use nail_api::{
    AccountId, AccountState, Address, Draft, ErrorCode, Event, FlagChange, FolderId, FolderRole,
    MessageSummary, MoveTarget, OutboxState, Request, Response, SearchQuery,
};
use nail_core::Service;
use nail_core::demo::{
    MULTI_HOME_EMAIL, MULTI_HOME_ID, MULTI_NEW_EMAIL, MULTI_NEW_PASSWORD, MULTI_WORK_EMAIL,
    MULTI_WORK_ID, start_demo_multi,
};
use serde_json::json;
use tokio::sync::broadcast;

type Fixture = Multi;

async fn setup() -> Fixture {
    Multi::start().await
}

async fn synced(f: &Fixture) {
    eventually("both accounts synced", || async {
        inbox_count(&f.service, MULTI_WORK_ID).await == 15
            && inbox_count(&f.service, MULTI_HOME_ID).await == 3
    })
    .await;
}

fn server_flags(f: &Fixture, mailbox: &str, subject: &str) -> Option<Vec<String>> {
    let list = f.control("imap-messages", &json!({ "mailbox": mailbox }));
    list.as_array()?
        .iter()
        .find(|m| m["subject"].as_str().unwrap_or_default().contains(subject))
        .map(|m| {
            m["flags"]
                .as_array()
                .unwrap()
                .iter()
                .map(|f| f.as_str().unwrap().to_owned())
                .collect()
        })
}

/// "Sync now" for the work account, then a moment for the pass.
async fn sync_and_settle(service: &Service) {
    request(
        service,
        Request::SyncNow {
            account_id: Some(AccountId::new(MULTI_WORK_ID)),
        },
    )
    .await;
    tokio::time::sleep(Duration::from_millis(500)).await;
}

#[tokio::test]
async fn configured_accounts_sync_and_list_with_all_accounts_first() {
    let f = setup().await;
    synced(&f).await;
    let Response::Accounts { accounts } = request(&f.service, Request::ListAccounts).await else {
        panic!()
    };
    let ids: Vec<&str> = accounts.iter().map(|a| a.id.as_str()).collect();
    // M3b / M3c: the demo Gmail and Exchange accounts are configured but
    // switched off.
    assert_eq!(
        ids,
        [
            "all",
            MULTI_WORK_ID,
            MULTI_HOME_ID,
            "gmail",
            "exchange",
            "corp"
        ]
    );
    assert_eq!(accounts[0].name, "");
    eventually("both accounts online", || async {
        statuses(&f.service).await
            == [
                (AccountId::new(MULTI_WORK_ID), AccountState::Online),
                (AccountId::new(MULTI_HOME_ID), AccountState::Online),
                (AccountId::new("gmail"), AccountState::Disabled),
                (AccountId::new("exchange"), AccountState::Disabled),
                (AccountId::new("corp"), AccountState::Disabled),
            ]
    })
    .await;

    // The unified inbox merges both inboxes and shows the mailing-list
    // message (in both accounts) once.
    let all_inbox = folder(&f.service, "all", FolderRole::Inbox).await.unwrap();
    assert_eq!(all_inbox.id.as_str(), "all/inbox");
    let unified = messages(&f.service, &all_inbox.id).await;
    assert_eq!(unified.len(), 15 + 3 - 1);
    let list: Vec<_> = unified
        .iter()
        .filter(|m| m.subject.contains("[nail-users]"))
        .collect();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].duplicates.len(), 1);
    // POP3 accounts have local folders.
    let home: Vec<FolderRole> = folders(&f.service, MULTI_HOME_ID)
        .await
        .iter()
        .map(|f| f.role)
        .collect();
    assert!(home.contains(&FolderRole::Trash) && home.contains(&FolderRole::Sent));
    f.service.shutdown().await;
}

#[tokio::test]
async fn writes_reach_the_imap_server() {
    let f = setup().await;
    synced(&f).await;
    let mut events = f.service.subscribe();

    // Flag: \Flagged on the server, OpStatus done.
    let welcome = find(
        &f.service,
        MULTI_WORK_ID,
        FolderRole::Inbox,
        "Welcome to Nail",
    )
    .await;
    let ops = op_ids(
        request(
            &f.service,
            Request::UpdateFlags {
                message_ids: vec![welcome.id.clone()],
                change: FlagChange {
                    flagged: Some(true),
                    ..FlagChange::default()
                },
            },
        )
        .await,
    );
    assert_eq!(ops.len(), 1);
    eventually("flag on the server", || async {
        server_flags(&f, "INBOX", "Welcome to Nail")
            .is_some_and(|flags| flags.iter().any(|x| x == "\\Flagged"))
    })
    .await;
    wait_op_done(&mut events, &ops[0]).await;

    // Move to Archive by role.
    let moved = find(
        &f.service,
        MULTI_WORK_ID,
        FolderRole::Inbox,
        "Welcome to Nail",
    )
    .await;
    op_ids(
        request(
            &f.service,
            Request::MoveMessages {
                message_ids: vec![moved.id.clone()],
                target: MoveTarget::Role(FolderRole::Archive),
            },
        )
        .await,
    );
    eventually("moved on the server", || async {
        server_flags(&f, "Archive", "Welcome to Nail").is_some()
            && server_flags(&f, "INBOX", "Welcome to Nail").is_none()
    })
    .await;
    // The local row keeps its id and learns its new UID; a later sync does
    // not duplicate it.
    request(
        &f.service,
        Request::SyncNow {
            account_id: Some(AccountId::new(MULTI_WORK_ID)),
        },
    )
    .await;
    eventually("archive synced once", || async {
        let archive = folder(&f.service, MULTI_WORK_ID, FolderRole::Archive)
            .await
            .unwrap();
        let rows: Vec<_> = messages(&f.service, &archive.id)
            .await
            .into_iter()
            .filter(|m| m.subject.contains("Welcome to Nail"))
            .collect();
        rows.len() == 1 && rows[0].id == moved.id
    })
    .await;

    // Delete: to Trash, then for good.
    let doomed = find(&f.service, MULTI_WORK_ID, FolderRole::Inbox, "[nail-users]").await;
    op_ids(
        request(
            &f.service,
            Request::DeleteMessages {
                message_ids: vec![doomed.id.clone()],
                permanent: false,
            },
        )
        .await,
    );
    eventually("in the server's Trash", || async {
        server_flags(&f, "Trash", "[nail-users]").is_some()
    })
    .await;
    let trashed = find(&f.service, MULTI_WORK_ID, FolderRole::Trash, "[nail-users]").await;
    eventually("the trashed copy has its UID", || async {
        f.service
            .store()
            .get_message(&trashed.id)
            .is_ok_and(|m| m.uid.is_some())
    })
    .await;
    op_ids(
        request(
            &f.service,
            Request::DeleteMessages {
                message_ids: vec![trashed.id.clone()],
                permanent: false,
            },
        )
        .await,
    );
    eventually("expunged from Trash", || async {
        server_flags(&f, "Trash", "[nail-users]").is_none()
    })
    .await;
    f.service.shutdown().await;
}

async fn wait_op_done(events: &mut broadcast::Receiver<Event>, op: &nail_api::OpId) {
    next_matching(events, "the op to finish", |event| match event {
        Event::OpStatus { status } if &status.op_id == op => match status.state {
            nail_api::OpState::Done => Some(()),
            nail_api::OpState::Failed => panic!("op failed: {status:?}"),
            _ => None,
        },
        _ => None,
    })
    .await;
}

#[tokio::test]
async fn server_flag_changes_arrive_through_idle() {
    let f = setup().await;
    synced(&f).await;
    assert!(
        f.demo.work.wait_for_idle(1, TIMEOUT).await,
        "the engine idles on INBOX"
    );
    let before = find(
        &f.service,
        MULTI_WORK_ID,
        FolderRole::Inbox,
        "Welcome to Nail",
    )
    .await;
    assert!(!before.flags.flagged);
    f.control(
        "imap-flags",
        &json!({ "subject": "Welcome to Nail", "flags": ["\\Flagged", "\\Seen"] }),
    );
    eventually("flag change pushed", || async {
        let m = find(
            &f.service,
            MULTI_WORK_ID,
            FolderRole::Inbox,
            "Welcome to Nail",
        )
        .await;
        m.flags.flagged && m.flags.seen
    })
    .await;
    // New mail arrives the same way (and is announced).
    let mut events = f.service.subscribe();
    f.control("imap-deliver",
        &json!({ "raw": "From: x@y.example\r\nSubject: Pushed by IDLE\r\nMessage-ID: <idle@y.example>\r\n\r\nhi" }),
    );
    // Announced after it is stored (and queued for the arrival rules): wait
    // for the announcement, reading the events as they come.
    next_matching(&mut events, "NewMail announced", |e| {
        matches!(e, Event::NewMail { .. }).then_some(())
    })
    .await;
    assert_eq!(inbox_count(&f.service, MULTI_WORK_ID).await, 16);
    // A dropped IDLE connection comes back.
    f.demo.work.drop_idle_connections();
    assert!(f.demo.work.wait_for_idle(1, TIMEOUT).await);
    f.service.shutdown().await;
}

#[tokio::test]
async fn writes_made_offline_replay_after_reconnecting() {
    let f = setup().await;
    synced(&f).await;
    go_offline(&f).await;
    let message = find(
        &f.service,
        MULTI_WORK_ID,
        FolderRole::Inbox,
        "Welcome to Nail",
    )
    .await;
    flag(&f.service, &message).await;
    // Applied locally at once, queued for the server.
    assert!(
        find(
            &f.service,
            MULTI_WORK_ID,
            FolderRole::Inbox,
            "Welcome to Nail"
        )
        .await
        .flags
        .flagged
    );
    assert_eq!(f.service.store().pending_op_count().unwrap(), 1);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        server_flags(&f, "INBOX", "Welcome to Nail")
            .is_some_and(|flags| !flags.iter().any(|x| x == "\\Flagged"))
    );

    f.control("imap-offline", &json!({ "offline": false }));
    eventually("replayed after reconnecting", || async {
        server_flags(&f, "INBOX", "Welcome to Nail")
            .is_some_and(|flags| flags.iter().any(|x| x == "\\Flagged"))
    })
    .await;
    eventually("queue empty", || async {
        f.service.store().pending_op_count().unwrap() == 0
    })
    .await;
    f.service.shutdown().await;
}

#[tokio::test]
async fn pop3_receives_and_deletes_by_policy() {
    let f = setup().await;
    synced(&f).await;
    f.control("pop3-deliver",
        &json!({ "raw": "From: grace@home.example\r\nSubject: Fresh from POP3\r\nMessage-ID: <fresh@home.example>\r\n\r\nnew", "uid": "home-3" }),
    );
    request(
        &f.service,
        Request::SyncNow {
            account_id: Some(AccountId::new(MULTI_HOME_ID)),
        },
    )
    .await;
    eventually("POP3 mail downloaded", || async {
        inbox_count(&f.service, MULTI_HOME_ID).await == 4
    })
    .await;
    // Deleting it for good deletes it on the server at the next fetch
    // (delete-when-deleted-locally).
    let fresh = find(
        &f.service,
        MULTI_HOME_ID,
        FolderRole::Inbox,
        "Fresh from POP3",
    )
    .await;
    op_ids(
        request(
            &f.service,
            Request::DeleteMessages {
                message_ids: vec![fresh.id],
                permanent: true,
            },
        )
        .await,
    );
    request(
        &f.service,
        Request::SyncNow {
            account_id: Some(AccountId::new(MULTI_HOME_ID)),
        },
    )
    .await;
    eventually("deleted on the POP3 server", || async {
        f.control("pop3-state", &json!({}))["deleted"]
            .as_array()
            .is_some_and(|d| d.iter().any(|u| u == "home-3"))
    })
    .await;
    assert_eq!(inbox_count(&f.service, MULTI_HOME_ID).await, 3);
    f.service.shutdown().await;
}

fn draft(identity: &str, to: &str, subject: &str) -> Draft {
    Draft {
        id: None,
        identity_id: identity.into(),
        to: vec![Address {
            name: Some("Bob".into()),
            email: to.into(),
        }],
        cc: vec![],
        bcc: vec![],
        subject: subject.into(),
        mode: ComposerMode::Plain,
        text: "Hello Bob,\nthis is Nail.".into(),
        html: None,
        markdown: None,
        attachments: vec![],
        context: None,
        signature: nail_api::SignatureChoice::Identity,
        markdown_part: false,
        automatic: None,
        crypto: None,
    }
}

/// M2a (ADRs 0029, 0034): a message that should be signed or encrypted is
/// refused with `no_key` when the keys are missing, never sent unprotected;
/// the choice comes from the request, the draft or the identity.
#[tokio::test]
async fn signed_or_encrypted_mail_is_refused_not_sent_plain() {
    use nail_api::security::{IdentityCrypto, SendCrypto};

    let f = setup().await;
    synced(&f).await;
    let sign = SendCrypto {
        sign: true,
        ..SendCrypto::none()
    };
    // Asked for in the request.
    let id = save(
        &f.service,
        draft(MULTI_WORK_EMAIL, "bob@example.org", "Signed"),
    )
    .await;
    let refused = request(
        &f.service,
        Request::SendDraft {
            draft_id: id.clone(),
            delay_secs: Some(0),
            send_at: None,
            crypto: Some(sign.clone()),
            expected_revision: None,
        },
    )
    .await;
    assert_eq!(error_of(&refused).0, ErrorCode::NoKey, "{refused:?}");
    // Kept with the draft.
    let encrypted = Draft {
        id: Some(id.clone()),
        crypto: Some(SendCrypto {
            encrypt: true,
            ..SendCrypto::none()
        }),
        ..draft(MULTI_WORK_EMAIL, "bob@example.org", "Signed")
    };
    save(&f.service, encrypted).await;
    let refused = request(
        &f.service,
        Request::SendDraft {
            draft_id: id.clone(),
            delay_secs: Some(0),
            send_at: None,
            crypto: None,
            expected_revision: None,
        },
    )
    .await;
    assert_eq!(error_of(&refused).0, ErrorCode::NoKey, "{refused:?}");
    // An explicit "neither" in the request wins over the draft.
    let Response::SendQueued { .. } = request(
        &f.service,
        Request::SendDraft {
            draft_id: id,
            delay_secs: Some(0),
            send_at: None,
            crypto: Some(SendCrypto::none()),
            expected_revision: None,
        },
    )
    .await
    else {
        panic!("expected the plain message to be queued")
    };

    // The identity's default (accounts.kdl `crypto sign=#true`).
    let Response::AccountsConfig { config } = request(&f.service, Request::GetAccountsConfig).await
    else {
        panic!()
    };
    let mut account = config
        .accounts
        .iter()
        .find(|a| a.id.as_str() == MULTI_WORK_ID)
        .unwrap()
        .clone();
    account.identities[0].crypto = Some(IdentityCrypto {
        sign: true,
        ..IdentityCrypto::default()
    });
    assert!(matches!(
        request(
            &f.service,
            Request::UpdateAccount {
                config: Box::new(account),
                secrets: vec![],
            }
        )
        .await,
        Response::AccountSaved { .. }
    ));
    let text = std::fs::read_to_string(f.config_dir.path().join("accounts.kdl")).unwrap();
    assert!(text.contains("crypto sign=#true"), "{text}");
    let id = save(
        &f.service,
        draft(MULTI_WORK_EMAIL, "bob@example.org", "Default"),
    )
    .await;
    let refused = request(
        &f.service,
        Request::SendDraft {
            draft_id: id,
            delay_secs: Some(0),
            send_at: None,
            crypto: None,
            expected_revision: None,
        },
    )
    .await;
    assert_eq!(error_of(&refused).0, ErrorCode::NoKey, "{refused:?}");
    assert!(!f.smtp_data().iter().any(|d| d.contains("Default")));
}

#[tokio::test]
async fn sending_with_undo_and_for_real() {
    let f = setup().await;
    synced(&f).await;
    let mut events = f.service.subscribe();

    // Undo: scheduled with a delay, cancelled before it is due.
    let undone = save(
        &f.service,
        draft(MULTI_WORK_EMAIL, "bob@example.org", "Cancelled by undo"),
    )
    .await;
    let Response::SendQueued { entry } = request(
        &f.service,
        Request::SendDraft {
            draft_id: undone.clone(),
            delay_secs: Some(30),
            send_at: None,
            crypto: None,
            expected_revision: None,
        },
    )
    .await
    else {
        panic!()
    };
    assert_eq!(entry.state, OutboxState::Scheduled);
    assert_eq!(
        request(
            &f.service,
            Request::CancelSend {
                outbox_id: entry.id.clone()
            }
        )
        .await,
        Response::SendCancelled {
            draft_id: undone.clone()
        }
    );
    // The draft is back and can be edited.
    assert!(matches!(
        request(&f.service, Request::GetDraft { draft_id: undone }).await,
        Response::Draft { .. }
    ));

    // For real: sent at once through the mock SMTP server.
    let sent = save(
        &f.service,
        draft(MULTI_WORK_EMAIL, "bob@example.org", "Sent for real"),
    )
    .await;
    let Response::SendQueued { entry } = request(
        &f.service,
        Request::SendDraft {
            draft_id: sent,
            delay_secs: Some(0),
            send_at: None,
            crypto: None,
            expected_revision: None,
        },
    )
    .await
    else {
        panic!()
    };
    eventually("SMTP received it", || async {
        f.smtp_data().iter().any(|d| d.contains("Sent for real"))
    })
    .await;
    assert!(
        !f.smtp_data()
            .iter()
            .any(|d| d.contains("Cancelled by undo"))
    );
    let data = f.smtp_data().remove(0);
    assert!(
        data.contains("From: \"Ada Lovelace\" <ada@work.example>"),
        "{data}"
    );
    // Plain text goes out as format=flowed (nail-compose, ADR 0023).
    assert!(data.contains("format=\"flowed\""), "{data}");
    let mut saw_sent = false;
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    while !saw_sent {
        let event = tokio::time::timeout_at(deadline, events.recv())
            .await
            .expect("SendStatus sent")
            .unwrap();
        if let Event::SendStatus {
            outbox_id, state, ..
        } = event
            && outbox_id == entry.id
        {
            assert_ne!(state, OutboxState::Failed);
            saw_sent = state == OutboxState::Sent;
        }
    }
    // The Sent copy went to the IMAP server and comes back by sync.
    eventually("Sent copy on the server", || async {
        server_flags(&f, "Sent", "Sent for real").is_some()
    })
    .await;
    eventually("Sent copy synced", || async {
        let sent = folder(&f.service, MULTI_WORK_ID, FolderRole::Sent)
            .await
            .unwrap();
        messages(&f.service, &sent.id)
            .await
            .iter()
            .any(|m| m.subject == "Sent for real")
    })
    .await;
    assert_eq!(
        request(&f.service, Request::ListOutbox).await,
        Response::Outbox { entries: vec![] }
    );

    // A POP3 account keeps its Sent copy locally.
    let home = save(
        &f.service,
        draft(MULTI_HOME_EMAIL, "grace@home.example", "From home"),
    )
    .await;
    request(
        &f.service,
        Request::SendDraft {
            draft_id: home,
            delay_secs: Some(0),
            send_at: None,
            crypto: None,
            expected_revision: None,
        },
    )
    .await;
    eventually("local Sent copy", || async {
        let sent = folder(&f.service, MULTI_HOME_ID, FolderRole::Sent)
            .await
            .unwrap();
        messages(&f.service, &sent.id)
            .await
            .iter()
            .any(|m| m.subject == "From home")
    })
    .await;
    f.service.shutdown().await;
}

#[tokio::test]
async fn search_threads_and_query_errors() {
    let f = setup().await;
    synced(&f).await;
    let search = |text: &str, account: Option<&str>| {
        let query = SearchQuery {
            text: text.into(),
            account_id: account.map(AccountId::new),
            folder_id: None,
            offset: 0,
            limit: 50,
            sort: nail_api::search::SearchSort::Date,
        };
        let service = f.service.clone();
        async move { request(&service, Request::Search { query }).await }
    };
    eventually("indexed", || async {
        let r = search("salad OR lake", None).await;
        matches!(r, Response::SearchResults { total, .. } if total == 2)
    })
    .await;
    match search("subject:\"notes for M1\"", Some("all")).await {
        Response::SearchResults { total, .. } => assert_eq!(total, 2, "one per account"),
        other => panic!("unexpected {other:?}"),
    }
    match search("subject:\"notes for M1\"", Some(MULTI_HOME_ID)).await {
        Response::SearchResults { total, .. } => assert_eq!(total, 1),
        other => panic!("unexpected {other:?}"),
    }
    match search("frm:grace", None).await {
        Response::InvalidQuery { error } => {
            assert_eq!(error.kind, nail_api::SearchErrorKind::UnknownField);
            assert_eq!((error.start, error.end), (0, 4));
        }
        other => panic!("unexpected {other:?}"),
    }
    assert!(matches!(
        request(&f.service, Request::CheckQuery { text: "(a".into() }).await,
        Response::QueryChecked { error: Some(_) }
    ));

    // Threads: the unified inbox lists threads, and GetThread returns the
    // mailing-list thread with its copy in the other account collapsed.
    let Response::Threads { threads, total, .. } = request(
        &f.service,
        Request::ListThreads {
            folder_id: FolderId::new("all/inbox"),
            offset: 0,
            limit: 100,
        },
    )
    .await
    else {
        panic!()
    };
    assert!(total > 0 && !threads.is_empty());
    let list = threads
        .iter()
        .find(|t| t.subject.contains("Release notes"))
        .unwrap();
    assert_eq!(list.message_count, 1);
    let Response::Thread { messages, .. } = request(
        &f.service,
        Request::GetThread {
            thread_id: list.id.clone(),
        },
    )
    .await
    else {
        panic!()
    };
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].duplicates.len(), 1);
    // P2 (ADR 0148): the conversation request answers for a listed folder
    // (copies collapsed, a mailing-list message is not the user's) and
    // refuses an unknown folder.
    let Response::Conversation { thread, messages } = request(
        &f.service,
        Request::GetConversation {
            thread_id: list.id.clone(),
            folder_id: Some(FolderId::new("all/inbox")),
        },
    )
    .await
    else {
        panic!()
    };
    assert_eq!(thread.id, list.id);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].message.duplicates.len(), 1);
    assert!(!messages[0].from_me);
    let refused = f
        .service
        .handle(Request::GetConversation {
            thread_id: list.id.clone(),
            folder_id: Some(FolderId::new("no-such-folder")),
        })
        .await;
    assert!(matches!(refused, Response::Error { .. }), "{refused:?}");
    f.service.shutdown().await;
}

#[tokio::test]
async fn accounts_are_discovered_tested_added_and_removed() {
    let f = setup().await;
    synced(&f).await;
    let Response::Discovered { candidates, .. } = request(
        &f.service,
        Request::DiscoverAccount {
            email: MULTI_NEW_EMAIL.into(),
            trusted_hosts: Vec::new(),
            windows_sign_in: false,
        },
    )
    .await
    else {
        panic!()
    };
    let candidate = candidates.first().expect("autoconfig found the servers");
    // The file lists POP3 first; IMAP is proposed first (ADR 0180).
    assert_eq!(candidate.incoming.protocol(), MailProtocol::Imap);
    assert_eq!(candidate.incoming.server().port, f.demo.new_imap.port());
    assert_eq!(
        candidates
            .iter()
            .map(|c| c.incoming.protocol())
            .collect::<Vec<_>>(),
        [MailProtocol::Imap, MailProtocol::Pop3]
    );

    let config = AccountConfig {
        id: AccountId::new(""),
        name: "Bob".into(),
        email: MULTI_NEW_EMAIL.into(),
        enabled: true,
        incoming: candidate.incoming.clone(),
        outgoing: candidate.outgoing.clone(),
        identities: vec![],
        sync: nail_api::accounts::SyncPolicy::default(),
        save_sent_copy: None,
        authserv_ids: Vec::new(),
        sieve: None,
    };
    let password = |p: &str| {
        vec![SecretInput {
            endpoint: EndpointSlot::Incoming,
            value: SecretValue::Password { password: p.into() },
        }]
    };
    let Response::AccountTested { result } = request(
        &f.service,
        Request::TestAccount {
            config: Box::new(config.clone()),
            secrets: password("wrong"),
        },
    )
    .await
    else {
        panic!()
    };
    assert_eq!(result.incoming.error, Some(ErrorCode::AuthFailed));
    let Response::AccountTested { result } = request(
        &f.service,
        Request::TestAccount {
            config: Box::new(config.clone()),
            secrets: password(MULTI_NEW_PASSWORD),
        },
    )
    .await
    else {
        panic!()
    };
    assert!(result.incoming.ok(), "{result:?}");
    assert!(result.incoming.capabilities.iter().any(|c| c == "IDLE"));
    assert!(
        result
            .outgoing
            .as_ref()
            .is_some_and(nail_api::accounts::EndpointTestResult::ok),
        "{result:?}"
    );

    let mut events = f.service.subscribe();
    let saved = match request(
        &f.service,
        Request::AddAccount {
            config: Box::new(config.clone()),
            secrets: password(MULTI_NEW_PASSWORD),
        },
    )
    .await
    {
        Response::AccountSaved { config } => config,
        other => panic!("unexpected {other:?}"),
    };
    assert_eq!(saved.id.as_str(), "bob-new.example");
    assert!(matches!(saved.incoming, IncomingServer::Imap { .. }));
    assert_eq!(saved.identities.len(), 1, "a default identity");
    let kdl = std::fs::read_to_string(f.config_dir.path().join("accounts.kdl")).unwrap();
    assert!(kdl.starts_with("// Demo accounts"), "comments kept");
    assert!(kdl.contains("bob-new.example"));
    assert!(!kdl.contains(MULTI_NEW_PASSWORD), "no secrets in the file");
    let mut changed = false;
    while let Ok(event) = events.try_recv() {
        changed |= event == Event::AccountsChanged;
    }
    assert!(changed);
    eventually("new account synced", || async {
        inbox_count(&f.service, "bob-new.example").await == 1
    })
    .await;
    // Adding it again under the same name is a conflict.
    let mut again = saved.as_ref().clone();
    again.identities.clear();
    assert!(matches!(
        request(
            &f.service,
            Request::AddAccount {
                config: Box::new(again),
                secrets: vec![]
            }
        )
        .await,
        Response::Error {
            code: ErrorCode::Conflict,
            ..
        }
    ));

    assert_eq!(
        request(
            &f.service,
            Request::RemoveAccount {
                account_id: saved.id.clone()
            }
        )
        .await,
        Response::Done
    );
    let kdl = std::fs::read_to_string(f.config_dir.path().join("accounts.kdl")).unwrap();
    assert!(!kdl.contains("bob-new.example"));
    // Left: the two demo accounts' passwords, the two demo DAV sources',
    // the demo Gmail account's sign-in (M3b) and the demo Exchange
    // accounts' sign-ins (M3c).
    assert_eq!(f.demo.secrets.len(), 7, "its keychain entry is gone");
    assert!(matches!(
        request(
            &f.service,
            Request::ListFolders {
                account_id: "bob-new.example".into()
            }
        )
        .await,
        Response::Error {
            code: ErrorCode::NotFound,
            ..
        }
    ));
    f.service.shutdown().await;
}

#[tokio::test]
async fn virtual_accounts_merge_folders_and_dedupe() {
    let f = setup().await;
    synced(&f).await;
    let Response::VirtualAccountSaved { config } = request(
        &f.service,
        Request::AddVirtualAccount {
            config: Box::new(VirtualAccountConfig {
                id: AccountId::new(""),
                name: "Everything".into(),
                members: vec![MULTI_WORK_ID.into(), MULTI_HOME_ID.into()],
                merge_roles: true,
                folder_mappings: vec![],
                default_identity: Some(MULTI_HOME_EMAIL.into()),
                identity_rules: vec![],
            }),
        },
    )
    .await
    else {
        panic!()
    };
    assert_eq!(config.id.as_str(), "everything");
    let inbox = folder(&f.service, "everything", FolderRole::Inbox)
        .await
        .unwrap();
    assert_eq!(inbox.id.as_str(), "everything/inbox");
    let list = messages(&f.service, &inbox.id).await;
    assert_eq!(list.len(), 17);
    assert_eq!(
        list.iter()
            .filter(|m| m.subject.contains("[nail-users]"))
            .count(),
        1
    );
    // Identities: the default first.
    let Response::Identities { identities } = request(
        &f.service,
        Request::ListIdentities {
            account_id: Some("everything".into()),
        },
    )
    .await
    else {
        panic!()
    };
    assert_eq!(identities[0].email, MULTI_HOME_EMAIL);
    assert_eq!(identities.len(), 2);
    // Moving into a virtual folder picks the member folder of the
    // message's own account.
    let archive = folder(&f.service, "everything", FolderRole::Archive)
        .await
        .unwrap();
    let home_mail = find(&f.service, MULTI_HOME_ID, FolderRole::Inbox, "Garden party").await;
    op_ids(
        request(
            &f.service,
            Request::MoveMessages {
                message_ids: vec![home_mail.id.clone()],
                target: MoveTarget::FolderId(archive.id.clone()),
            },
        )
        .await,
    );
    let moved = f.service.store().get_message(&home_mail.id).unwrap();
    let home_archive = folder(&f.service, MULTI_HOME_ID, FolderRole::Archive)
        .await
        .unwrap();
    assert_eq!(moved.summary.folder_id, home_archive.id);
    assert_eq!(
        request(
            &f.service,
            Request::RemoveVirtualAccount {
                account_id: "everything".into()
            }
        )
        .await,
        Response::Done
    );
    f.service.shutdown().await;
}

async fn chosen(
    service: &Service,
    account: Option<&str>,
    recipients: &[&str],
    reply_to: Option<&nail_api::MessageId>,
) -> Option<String> {
    match request(
        service,
        Request::ChooseIdentity {
            account_id: account.map(AccountId::new),
            recipients: recipients.iter().map(|r| (*r).to_owned()).collect(),
            reply_to: reply_to.cloned(),
        },
    )
    .await
    {
        Response::IdentityChosen { identity_id } => identity_id.map(|i| i.as_str().to_owned()),
        other => panic!("unexpected {other:?}"),
    }
}

/// Review M1a: the sending identity (virtual-account rules, the identity a
/// reply goes out from, the default) is chosen by the core, for every
/// client alike.
#[tokio::test]
async fn the_core_chooses_the_sending_identity() {
    let f = setup().await;
    synced(&f).await;
    request(
        &f.service,
        Request::AddVirtualAccount {
            config: Box::new(VirtualAccountConfig {
                id: AccountId::new("both"),
                name: "Both".into(),
                members: vec![MULTI_WORK_ID.into(), MULTI_HOME_ID.into()],
                merge_roles: true,
                folder_mappings: vec![],
                default_identity: Some(MULTI_WORK_EMAIL.into()),
                identity_rules: vec![nail_api::accounts::IdentityRule {
                    pattern: "*@home.example".into(),
                    identity: MULTI_HOME_EMAIL.into(),
                }],
            }),
        },
    )
    .await;
    let s = &f.service;
    // Defaults: the virtual account's default identity; a real account's own.
    assert_eq!(
        chosen(s, Some("both"), &[], None).await.as_deref(),
        Some(MULTI_WORK_EMAIL)
    );
    assert_eq!(
        chosen(s, Some(MULTI_HOME_ID), &[], None).await.as_deref(),
        Some(MULTI_HOME_EMAIL)
    );
    // A rule matching any recipient decides (case-insensitively).
    assert_eq!(
        chosen(
            s,
            Some("both"),
            &["bob@example.org", "Grace@HOME.example"],
            None
        )
        .await
        .as_deref(),
        Some(MULTI_HOME_EMAIL)
    );
    // Rules belong to the virtual account only.
    assert_eq!(
        chosen(s, Some(MULTI_WORK_ID), &["grace@home.example"], None)
            .await
            .as_deref(),
        Some(MULTI_WORK_EMAIL)
    );
    // A reply goes out from the identity the original was sent to.
    let garden = find(s, MULTI_HOME_ID, FolderRole::Inbox, "Garden party").await;
    assert_eq!(
        chosen(
            s,
            Some(MULTI_WORK_ID),
            &["grace@home.example"],
            Some(&garden.id)
        )
        .await
        .as_deref(),
        Some(MULTI_HOME_EMAIL)
    );
    // An unknown original falls back to the default.
    assert_eq!(
        chosen(s, Some(MULTI_WORK_ID), &[], Some(&"m-unknown".into()))
            .await
            .as_deref(),
        Some(MULTI_WORK_EMAIL)
    );
    f.service.shutdown().await;
}

#[tokio::test]
async fn hand_edits_of_accounts_kdl_apply_or_report_errors() {
    let f = setup().await;
    synced(&f).await;
    let mut events = f.service.subscribe();
    let path = f.config_dir.path().join("accounts.kdl");
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, text.replace("name=\"Home\"", "name=\"Home mail\"")).unwrap();
    eventually("renamed", || async {
        let Response::Accounts { accounts } = request(&f.service, Request::ListAccounts).await
        else {
            return false;
        };
        accounts.iter().any(|a| a.name == "Home mail")
    })
    .await;
    std::fs::write(&path, "account \"broken\" {").unwrap();
    let file = next_matching(&mut events, "an accounts.kdl error", |e| match e {
        Event::ConfigError { file, .. } => Some(file),
        _ => None,
    })
    .await;
    assert_eq!(file, "accounts.kdl");
    // The last valid accounts stay, and edits are refused meanwhile.
    assert_eq!(inbox_count(&f.service, MULTI_HOME_ID).await, 3);
    assert!(matches!(
        request(
            &f.service,
            Request::RemoveAccount {
                account_id: MULTI_HOME_ID.into()
            }
        )
        .await,
        Response::Error {
            code: ErrorCode::Config,
            ..
        }
    ));
    assert!(
        f.service
            .config_error_events()
            .iter()
            .any(|e| matches!(e, Event::ConfigError { file, .. } if file == "accounts.kdl"))
    );
    f.service.shutdown().await;
}

#[tokio::test]
async fn failed_sends_are_retried_or_reported() {
    let f = setup().await;
    synced(&f).await;
    let mut events = f.service.subscribe();

    // A permanent refusal: failed, kept in the outbox with the reason.
    f.demo
        .smtp
        .reject_recipient("nobody@example.org", 550, "5.1.1 no such user");
    let rejected = save(
        &f.service,
        draft(MULTI_WORK_EMAIL, "nobody@example.org", "Refused"),
    )
    .await;
    request(
        &f.service,
        Request::SendDraft {
            draft_id: rejected,
            delay_secs: Some(0),
            send_at: None,
            crypto: None,
            expected_revision: None,
        },
    )
    .await;
    let failed = send_outcome(&mut events).await;
    assert_eq!(failed.0, OutboxState::Failed);
    assert_eq!(failed.1, Some(ErrorCode::Rejected));
    let Response::Outbox { entries } = request(&f.service, Request::ListOutbox).await else {
        panic!()
    };
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].state, OutboxState::Failed);

    // A temporary failure: scheduled again, then sent.
    f.demo
        .smtp
        .fail("MAIL", 451, "4.3.0 try again later", Some(1));
    let flaky = save(
        &f.service,
        draft(MULTI_WORK_EMAIL, "bob@example.org", "Second try"),
    )
    .await;
    request(
        &f.service,
        Request::SendDraft {
            draft_id: flaky,
            delay_secs: Some(0),
            send_at: None,
            crypto: None,
            expected_revision: None,
        },
    )
    .await;
    let retry = send_outcome(&mut events).await;
    assert_eq!(retry, (OutboxState::Scheduled, Some(ErrorCode::Network)));
    let sent = send_outcome(&mut events).await;
    assert_eq!(sent, (OutboxState::Sent, None));
    assert!(f.smtp_data().iter().any(|d| d.contains("Second try")));

    // Drafts need a known identity; attachments must be uploaded blobs.
    let mut bad = draft("nobody@nowhere.example", "bob@example.org", "x");
    assert!(matches!(
        request(&f.service, Request::SaveDraft { draft: bad.clone() }).await,
        Response::Error {
            code: ErrorCode::InvalidRequest,
            ..
        }
    ));
    bad.identity_id = MULTI_WORK_EMAIL.into();
    bad.attachments = vec![nail_api::DraftAttachment {
        filename: "a.pdf".into(),
        mime: "application/pdf".into(),
        size: 1,
        blob: "00".repeat(32),
        content_id: None,
    }];
    assert!(matches!(
        request(&f.service, Request::SaveDraft { draft: bad.clone() }).await,
        Response::Error {
            code: ErrorCode::NotFound,
            ..
        }
    ));
    // Rich drafts are stored, with the HTML sanitized.
    bad.attachments.clear();
    bad.mode = ComposerMode::Html;
    bad.html = Some("<p onclick=\"x()\">x</p><script>bad()</script>".into());
    let Response::DraftSaved { draft_id } =
        request(&f.service, Request::SaveDraft { draft: bad }).await
    else {
        panic!("rich draft refused")
    };
    let Response::Draft { draft } = request(&f.service, Request::GetDraft { draft_id }).await
    else {
        panic!()
    };
    let html = draft.html.unwrap();
    assert!(html.contains("<p>x</p>"), "{html}");
    assert!(
        !html.contains("script") && !html.contains("onclick"),
        "{html}"
    );
    f.service.shutdown().await;
}

/// The next final (non-`sending`) `SendStatus`.
async fn send_outcome(events: &mut broadcast::Receiver<Event>) -> (OutboxState, Option<ErrorCode>) {
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    loop {
        let event = tokio::time::timeout_at(deadline, events.recv())
            .await
            .expect("a send result in time")
            .unwrap();
        if let Event::SendStatus { state, error, .. } = event
            && state != OutboxState::Sending
            && !(state == OutboxState::Scheduled && error.is_none())
        {
            return (state, error);
        }
    }
}

#[tokio::test]
async fn tags_and_cross_account_moves_replay() {
    let f = setup().await;
    synced(&f).await;
    let welcome = find(
        &f.service,
        MULTI_WORK_ID,
        FolderRole::Inbox,
        "Welcome to Nail",
    )
    .await;
    op_ids(
        request(
            &f.service,
            Request::UpdateFlags {
                message_ids: vec![welcome.id.clone()],
                change: FlagChange {
                    add_tags: vec!["$label1".into()],
                    ..FlagChange::default()
                },
            },
        )
        .await,
    );
    eventually("keyword on the server", || async {
        server_flags(&f, "INBOX", "Welcome to Nail")
            .is_some_and(|flags| flags.iter().any(|x| x == "$label1"))
    })
    .await;
    assert!(matches!(
        request(
            &f.service,
            Request::UpdateFlags {
                message_ids: vec![welcome.id.clone()],
                change: FlagChange {
                    add_tags: vec!["bad tag".into()],
                    ..FlagChange::default()
                },
            },
        )
        .await,
        Response::Error {
            code: ErrorCode::InvalidRequest,
            ..
        }
    ));

    // Into the POP3 account: the local copy moves, the IMAP server drops it.
    let home_archive = folder(&f.service, MULTI_HOME_ID, FolderRole::Archive)
        .await
        .unwrap();
    op_ids(
        request(
            &f.service,
            Request::MoveMessages {
                message_ids: vec![welcome.id.clone()],
                target: MoveTarget::FolderId(home_archive.id.clone()),
            },
        )
        .await,
    );
    eventually("gone from the IMAP server", || async {
        server_flags(&f, "INBOX", "Welcome to Nail").is_none()
    })
    .await;
    let moved = f.service.store().get_message(&welcome.id).unwrap();
    assert_eq!(moved.summary.folder_id, home_archive.id);
    assert_eq!(moved.summary.account_id, AccountId::new(MULTI_HOME_ID));
    // The body is still there.
    assert!(matches!(
        request(&f.service, Request::GetMessage { id: welcome.id }).await,
        Response::Message { .. }
    ));
    f.service.shutdown().await;
}

#[tokio::test]
async fn send_later_waits_for_its_time() {
    let f = setup().await;
    synced(&f).await;
    let later = save(
        &f.service,
        draft(MULTI_HOME_EMAIL, "grace@home.example", "Tomorrow"),
    )
    .await;
    let at = "2099-01-01T09:00:00Z";
    let Response::SendQueued { entry } = request(
        &f.service,
        Request::SendDraft {
            draft_id: later.clone(),
            delay_secs: None,
            send_at: Some(at.into()),
            crypto: None,
            expected_revision: None,
        },
    )
    .await
    else {
        panic!()
    };
    assert_eq!(entry.send_at, at);
    assert_eq!(entry.account_id, AccountId::new(MULTI_HOME_ID));
    let Response::Outbox { entries } = request(&f.service, Request::ListOutbox).await else {
        panic!()
    };
    assert_eq!(entries.len(), 1);
    // A scheduled draft cannot be changed; cancelling makes it a draft again.
    assert!(matches!(
        request(
            &f.service,
            Request::DeleteDraft {
                draft_id: later.clone()
            }
        )
        .await,
        Response::Error {
            code: ErrorCode::Conflict,
            ..
        }
    ));
    assert_eq!(
        request(
            &f.service,
            Request::CancelSend {
                outbox_id: entry.id.clone()
            }
        )
        .await,
        Response::SendCancelled {
            draft_id: later.clone()
        }
    );
    assert!(matches!(
        request(
            &f.service,
            Request::SendDraft {
                draft_id: later.clone(),
                delay_secs: None,
                send_at: Some("tomorrow".into()),
                crypto: None,
                expected_revision: None,
            },
        )
        .await,
        Response::Error {
            code: ErrorCode::InvalidRequest,
            ..
        }
    ));
    assert_eq!(
        request(&f.service, Request::DeleteDraft { draft_id: later }).await,
        Response::Done
    );
    assert_eq!(f.smtp_data().len(), 0);
    f.service.shutdown().await;
}

async fn go_offline(f: &Fixture) {
    f.control("imap-offline", &json!({ "offline": true }));
    eventually("work offline", || async {
        statuses(&f.service)
            .await
            .contains(&(AccountId::new(MULTI_WORK_ID), AccountState::Offline))
    })
    .await;
}

fn count(subjects: &[String], subject: &str) -> usize {
    subjects.iter().filter(|s| s.contains(subject)).count()
}

async fn queue_empty(f: &Fixture) {
    eventually("queue empty", || async {
        f.service.store().pending_op_count().unwrap() == 0
    })
    .await;
}

async fn delete(service: &Service, message: &MessageSummary, permanent: bool) {
    op_ids(
        request(
            service,
            Request::DeleteMessages {
                message_ids: vec![message.id.clone()],
                permanent,
            },
        )
        .await,
    );
}

async fn move_to(service: &Service, message: &MessageSummary, role: FolderRole) {
    op_ids(
        request(
            service,
            Request::MoveMessages {
                message_ids: vec![message.id.clone()],
                target: MoveTarget::Role(role),
            },
        )
        .await,
    );
}

async fn flag(service: &Service, message: &MessageSummary) {
    op_ids(
        request(
            service,
            Request::UpdateFlags {
                message_ids: vec![message.id.clone()],
                change: FlagChange {
                    flagged: Some(true),
                    ..FlagChange::default()
                },
            },
        )
        .await,
    );
}

async fn local_subjects(service: &Service, role: FolderRole) -> Vec<String> {
    let folder = folder(service, MULTI_WORK_ID, role).await.unwrap();
    messages(service, &folder.id)
        .await
        .into_iter()
        .map(|m| m.subject)
        .collect()
}

/// Review M1a: a queued delete / move / flag change must not act on a UID
/// that the server reassigned (UIDVALIDITY change) while it waited.
#[tokio::test]
async fn queued_writes_survive_a_uidvalidity_change() {
    let f = setup().await;
    synced(&f).await;
    go_offline(&f).await;
    let photo = find(
        &f.service,
        MULTI_WORK_ID,
        FolderRole::Inbox,
        "Photo of the new lab bench",
    )
    .await;
    let invoice = find(&f.service, MULTI_WORK_ID, FolderRole::Inbox, "Your invoice").await;
    let welcome = find(
        &f.service,
        MULTI_WORK_ID,
        FolderRole::Inbox,
        "Welcome to Nail",
    )
    .await;
    delete(&f.service, &photo, true).await;
    move_to(&f.service, &invoice, FolderRole::Archive).await;
    flag(&f.service, &welcome).await;

    // Meanwhile the server rebuilds INBOX: one message goes, the rest is
    // renumbered from 1 under a new UIDVALIDITY.
    let inbox = f.demo.work.store().mailbox("INBOX").unwrap();
    // The first message that none of the writes is about (the newsletter,
    // UID 2): every later UID shifts down by one.
    let first = inbox
        .messages
        .iter()
        .find(|m| {
            let raw = String::from_utf8_lossy(&m.raw);
            !["lab bench", "invoice", "Welcome to Nail"]
                .iter()
                .any(|t| raw.contains(t))
        })
        .unwrap();
    f.demo.work.store().expunge("INBOX", first.uid);
    f.demo.work.store().reset_uid_validity("INBOX").unwrap();
    let before = f.server_subjects(MULTI_WORK_ID, "INBOX");
    assert_eq!(count(&before, "Grüße aus München"), 1);

    f.control("imap-offline", &json!({ "offline": false }));
    queue_empty(&f).await;
    eventually("the right messages changed on the server", || async {
        let inbox = f.server_subjects(MULTI_WORK_ID, "INBOX");
        count(&inbox, "Photo of the new lab bench") == 0
            && count(&inbox, "Your invoice") == 0
            && count(&f.server_subjects(MULTI_WORK_ID, "Archive"), "Your invoice") == 1
    })
    .await;
    let after = f.server_subjects(MULTI_WORK_ID, "INBOX");
    assert_eq!(after.len(), before.len() - 2, "{after:?}");
    assert_eq!(count(&after, "Grüße aus München"), 1, "{after:?}");
    assert_eq!(count(&after, "Welcome to Nail"), 1, "{after:?}");
    let flagged: Vec<String> = f
        .demo
        .work
        .store()
        .mailbox("INBOX")
        .unwrap()
        .messages
        .iter()
        .filter(|m| m.flags.iter().any(|x| x == "\\Flagged"))
        .map(|m| String::from_utf8_lossy(&m.raw).into_owned())
        .collect();
    assert!(
        flagged.iter().any(|raw| raw.contains("Welcome to Nail")),
        "the flag went missing"
    );
    let flagged_before = inbox
        .messages
        .iter()
        .filter(|m| m.flags.iter().any(|x| x == "\\Flagged"))
        .count();
    assert!(
        flagged.len() <= flagged_before + 1,
        "a renumbered message got the flag: {} flagged",
        flagged.len()
    );
    f.service.shutdown().await;
}

/// Review M1a: moving (or deleting) a message again before the first move
/// reached the server must not be dropped.
#[tokio::test]
async fn moves_and_deletes_made_before_replay_are_all_applied() {
    let f = setup().await;
    synced(&f).await;
    go_offline(&f).await;
    // (A) Archive, then delete for good.
    let welcome = find(
        &f.service,
        MULTI_WORK_ID,
        FolderRole::Inbox,
        "Welcome to Nail",
    )
    .await;
    move_to(&f.service, &welcome, FolderRole::Archive).await;
    delete(&f.service, &welcome, true).await;
    // (B) Archive, then to Trash.
    let photo = find(
        &f.service,
        MULTI_WORK_ID,
        FolderRole::Inbox,
        "Photo of the new lab bench",
    )
    .await;
    move_to(&f.service, &photo, FolderRole::Archive).await;
    delete(&f.service, &photo, false).await;
    // (C) Archive, flag, then back to INBOX.
    let invoice = find(&f.service, MULTI_WORK_ID, FolderRole::Inbox, "Your invoice").await;
    move_to(&f.service, &invoice, FolderRole::Archive).await;
    flag(&f.service, &invoice).await;
    move_to(&f.service, &invoice, FolderRole::Inbox).await;

    f.control("imap-offline", &json!({ "offline": false }));
    queue_empty(&f).await;
    let failed = f
        .service
        .store()
        .list_ops(Some(nail_store::OpState::Failed))
        .unwrap();
    assert!(failed.is_empty(), "{failed:?}");
    let (inbox, archive, trash) = (
        f.server_subjects(MULTI_WORK_ID, "INBOX"),
        f.server_subjects(MULTI_WORK_ID, "Archive"),
        f.server_subjects(MULTI_WORK_ID, "Trash"),
    );
    for mailbox in [&inbox, &archive, &trash] {
        assert_eq!(count(mailbox, "Welcome to Nail"), 0, "{mailbox:?}");
    }
    assert_eq!(count(&trash, "Photo of the new lab bench"), 1);
    assert_eq!(count(&archive, "Photo of the new lab bench"), 0);
    assert_eq!(count(&inbox, "Your invoice"), 1);
    assert_eq!(count(&archive, "Your invoice"), 0);
    assert!(
        server_flags(&f, "INBOX", "Your invoice")
            .is_some_and(|flags| flags.iter().any(|x| x == "\\Flagged"))
    );

    // After a sync every message shows once, where the user put it.
    sync_and_settle(&f.service).await;
    eventually("local folders match", || async {
        let inbox = local_subjects(&f.service, FolderRole::Inbox).await;
        let archive = local_subjects(&f.service, FolderRole::Archive).await;
        let trash = local_subjects(&f.service, FolderRole::Trash).await;
        [&inbox, &archive, &trash]
            .iter()
            .all(|m| count(m, "Welcome to Nail") == 0)
            && count(&trash, "Photo of the new lab bench") == 1
            && count(&archive, "Photo of the new lab bench") == 0
            && count(&inbox, "Your invoice") == 1
            && count(&archive, "Your invoice") == 0
    })
    .await;
    let trashed = f.service.store().get_message(&photo.id).unwrap();
    assert!(trashed.uid.is_some(), "the trashed row learned its UID");
    f.service.shutdown().await;
}

/// Review M1a: a server that accepts the login but fails the session right
/// away must not get a full reconnect-and-sync every `reconnect_min`.
#[tokio::test]
async fn a_session_failing_right_after_login_backs_off() {
    let f = setup().await;
    synced(&f).await;
    // Every session now breaks at its first LIST (the start of a pass).
    f.demo.work.disconnect_on(Some("LIST"));
    request(
        &f.service,
        Request::SyncNow {
            account_id: Some(AccountId::new(MULTI_WORK_ID)),
        },
    )
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let before = f.demo.work.connection_count();
    tokio::time::sleep(Duration::from_secs(3)).await;
    let attempts = f.demo.work.connection_count() - before;
    // Growing delays (0.4 s, 0.8 s, then 1 s): about four attempts in 3 s;
    // resetting to 0.2 s after every login would make it a dozen or more.
    assert!(attempts <= 6, "{attempts} reconnects in 3 s");
    f.demo.work.disconnect_on(None);
    f.service.shutdown().await;
}

/// A demo-multi fixture with a third, IMAP account "new" (the discoverable
/// mock server).
async fn setup_with_new_account() -> Fixture {
    use nail_api::engine::SecretStore;
    let demo = start_demo_multi().await.unwrap();
    Multi::with(demo, |demo, config_dir| {
        let kdl = format!(
            "{}\naccount \"new\" email=\"{MULTI_NEW_EMAIL}\" name=\"New\" {{\n    imap host=\"127.0.0.1\" port={} tls=\"plaintext\"\n}}\n",
            demo.accounts_kdl(),
            demo.new_imap.port()
        );
        std::fs::write(config_dir.join("accounts.kdl"), kdl).unwrap();
        demo.secrets
            .set(
                &nail_api::SecretRef::default_for(
                    &AccountId::new("new"),
                    nail_api::accounts::SecretKind::Password,
                ),
                MULTI_NEW_PASSWORD,
            )
            .unwrap();
    })
}

/// Review M1a: a cross-account move whose source step fails after the copy
/// was filed must not file it again on retry.
#[tokio::test]
async fn a_retried_cross_account_move_files_one_copy() {
    let f = setup_with_new_account().await;
    synced(&f).await;
    eventually("new synced", || async {
        inbox_count(&f.service, "new").await == 1
    })
    .await;
    let new_inbox = folder(&f.service, "new", FolderRole::Inbox).await.unwrap();
    let welcome = find(
        &f.service,
        MULTI_WORK_ID,
        FolderRole::Inbox,
        "Welcome to Nail",
    )
    .await;
    // The source's expunge breaks the connection (a retryable failure)
    // after the copy reached the target.
    f.demo.work.disconnect_on(Some("UID EXPUNGE"));
    op_ids(
        request(
            &f.service,
            Request::MoveMessages {
                message_ids: vec![welcome.id.clone()],
                target: MoveTarget::FolderId(new_inbox.id.clone()),
            },
        )
        .await,
    );
    eventually("copy filed in the target", || async {
        count(&f.server_subjects("new", "INBOX"), "Welcome to Nail") == 1
    })
    .await;
    // Let the engine fail and retry a few times.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(f.service.store().pending_op_count().unwrap(), 1);
    f.demo.work.disconnect_on(None);
    queue_empty(&f).await;
    eventually("gone from the source", || async {
        count(
            &f.server_subjects(MULTI_WORK_ID, "INBOX"),
            "Welcome to Nail",
        ) == 0
    })
    .await;
    assert_eq!(
        count(&f.server_subjects("new", "INBOX"), "Welcome to Nail"),
        1,
        "the retry filed another copy"
    );
    f.service.shutdown().await;
}

/// Review M1a: the Sent copy goes through the operation queue, so an IMAP
/// outage while sending does not lose it.
#[tokio::test]
async fn the_sent_copy_waits_for_the_imap_server() {
    const SUBJECT: &str = "Sent while IMAP is down";
    let f = setup().await;
    synced(&f).await;
    go_offline(&f).await;
    let sent = save(
        &f.service,
        draft(MULTI_WORK_EMAIL, "bob@example.org", SUBJECT),
    )
    .await;
    request(
        &f.service,
        Request::SendDraft {
            draft_id: sent,
            delay_secs: Some(0),
            send_at: None,
            crypto: None,
            expected_revision: None,
        },
    )
    .await;
    eventually("SMTP received it", || async {
        f.smtp_data().iter().any(|d| d.contains(SUBJECT))
    })
    .await;
    eventually("local Sent copy at once", || async {
        count(&local_subjects(&f.service, FolderRole::Sent).await, SUBJECT) == 1
    })
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(count(&f.server_subjects(MULTI_WORK_ID, "Sent"), SUBJECT), 0);
    assert!(f.service.store().pending_op_count().unwrap() >= 1);

    f.control("imap-offline", &json!({ "offline": false }));
    eventually("filed on the server", || async {
        count(&f.server_subjects(MULTI_WORK_ID, "Sent"), SUBJECT) == 1
    })
    .await;
    queue_empty(&f).await;
    sync_and_settle(&f.service).await;
    assert_eq!(
        count(&local_subjects(&f.service, FolderRole::Sent).await, SUBJECT),
        1,
        "the synced copy is the local one, not a second row"
    );
    let sent_folder = folder(&f.service, MULTI_WORK_ID, FolderRole::Sent)
        .await
        .unwrap();
    let copy = messages(&f.service, &sent_folder.id)
        .await
        .into_iter()
        .find(|m| m.subject == SUBJECT)
        .unwrap();
    assert!(
        f.service
            .store()
            .get_message(&copy.id)
            .unwrap()
            .uid
            .is_some()
    );
    assert_eq!(count(&f.server_subjects(MULTI_WORK_ID, "Sent"), SUBJECT), 1);
    f.service.shutdown().await;
}
