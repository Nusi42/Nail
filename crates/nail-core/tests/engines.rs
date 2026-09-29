//! The M1a core end to end against the `demo-multi` mock servers: account
//! engines (IMAP with IDLE, POP3), the operation queue replayed to the
//! servers, offline replay, sending with undo, virtual accounts, search,
//! threads and account setup.

use std::future::Future;
use std::time::Duration;

use nail_api::accounts::{
    AccountConfig, ComposerMode, EndpointSlot, IncomingServer, SecretInput, SecretValue,
    VirtualAccountConfig,
};
use nail_api::{
    AccountId, AccountState, Address, Draft, ErrorCode, Event, FlagChange, Folder, FolderId,
    FolderRole, MessageSummary, MoveTarget, OutboxState, Request, Response, SearchQuery,
};
use nail_core::demo::{
    DemoMulti, MULTI_HOME_EMAIL, MULTI_HOME_ID, MULTI_NEW_EMAIL, MULTI_NEW_PASSWORD,
    MULTI_WORK_EMAIL, MULTI_WORK_ID, start_demo_multi,
};
use nail_core::{Service, ServiceOptions};
use serde_json::json;
use tempfile::TempDir;
use tokio::sync::broadcast;

const TIMEOUT: Duration = Duration::from_secs(20);

struct Fixture {
    service: Service,
    demo: DemoMulti,
    config_dir: TempDir,
    _data_dir: TempDir,
}

async fn setup() -> Fixture {
    let demo = start_demo_multi().await.unwrap();
    let data_dir = tempfile::tempdir().unwrap();
    let config_dir = tempfile::tempdir().unwrap();
    demo.write_config(config_dir.path()).unwrap();
    let options = demo.service_options(ServiceOptions::new(data_dir.path(), config_dir.path()));
    let service = Service::open(options).unwrap();
    Fixture {
        service,
        demo,
        config_dir,
        _data_dir: data_dir,
    }
}

async fn eventually<F, Fut>(what: &str, mut check: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    loop {
        if check().await {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for: {what}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn request(service: &Service, request: Request) -> Response {
    service.handle(request).await
}

async fn folders(service: &Service, account: &str) -> Vec<Folder> {
    match request(
        service,
        Request::ListFolders {
            account_id: AccountId::new(account),
        },
    )
    .await
    {
        Response::Folders { folders } => folders,
        other => panic!("unexpected {other:?}"),
    }
}

async fn folder(service: &Service, account: &str, role: FolderRole) -> Option<Folder> {
    folders(service, account)
        .await
        .into_iter()
        .find(|f| f.role == role)
}

async fn messages(service: &Service, folder: &FolderId) -> Vec<MessageSummary> {
    match request(
        service,
        Request::ListMessages {
            folder_id: folder.clone(),
            offset: 0,
            limit: 500,
        },
    )
    .await
    {
        Response::Messages { messages, .. } => messages,
        other => panic!("unexpected {other:?}"),
    }
}

async fn inbox_count(service: &Service, account: &str) -> usize {
    match folder(service, account, FolderRole::Inbox).await {
        Some(f) => messages(service, &f.id).await.len(),
        None => 0,
    }
}

async fn synced(f: &Fixture) {
    eventually("both accounts synced", || async {
        inbox_count(&f.service, MULTI_WORK_ID).await == 15
            && inbox_count(&f.service, MULTI_HOME_ID).await == 3
    })
    .await;
}

async fn find(service: &Service, account: &str, role: FolderRole, subject: &str) -> MessageSummary {
    let folder = folder(service, account, role).await.unwrap();
    messages(service, &folder.id)
        .await
        .into_iter()
        .find(|m| m.subject.contains(subject))
        .unwrap_or_else(|| panic!("no message {subject:?} in {account} {role:?}"))
}

fn control(f: &Fixture, command: &str, args: serde_json::Value) -> serde_json::Value {
    f.demo.control(command, &args).unwrap()
}

fn server_flags(f: &Fixture, mailbox: &str, subject: &str) -> Option<Vec<String>> {
    let list = control(f, "imap-messages", json!({ "mailbox": mailbox }));
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

async fn statuses(service: &Service) -> Vec<(AccountId, AccountState)> {
    match request(service, Request::ListAccountStatuses).await {
        Response::AccountStatuses { statuses } => statuses
            .into_iter()
            .map(|s| (s.account_id, s.state))
            .collect(),
        other => panic!("unexpected {other:?}"),
    }
}

fn op_ids(response: Response) -> Vec<nail_api::OpId> {
    match response {
        Response::Queued { op_ids } => op_ids,
        other => panic!("expected Queued, got {other:?}"),
    }
}

#[tokio::test]
async fn configured_accounts_sync_and_list_with_all_accounts_first() {
    let f = setup().await;
    synced(&f).await;
    let Response::Accounts { accounts } = request(&f.service, Request::ListAccounts).await else {
        panic!()
    };
    let ids: Vec<&str> = accounts.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, ["all", MULTI_WORK_ID, MULTI_HOME_ID]);
    assert_eq!(accounts[0].name, "");
    eventually("both accounts online", || async {
        statuses(&f.service).await
            == [
                (AccountId::new(MULTI_WORK_ID), AccountState::Online),
                (AccountId::new(MULTI_HOME_ID), AccountState::Online),
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
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    loop {
        let event = tokio::time::timeout_at(deadline, events.recv())
            .await
            .expect("op finished in time")
            .unwrap();
        if let Event::OpStatus { status } = event
            && &status.op_id == op
        {
            match status.state {
                nail_api::OpState::Done => return,
                nail_api::OpState::Failed => panic!("op failed: {status:?}"),
                _ => {}
            }
        }
    }
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
    control(
        &f,
        "imap-flags",
        json!({ "subject": "Welcome to Nail", "flags": ["\\Flagged", "\\Seen"] }),
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
    control(
        &f,
        "imap-deliver",
        json!({ "raw": "From: x@y.example\r\nSubject: Pushed by IDLE\r\nMessage-ID: <idle@y.example>\r\n\r\nhi" }),
    );
    eventually("new mail pushed", || async {
        inbox_count(&f.service, MULTI_WORK_ID).await == 16
    })
    .await;
    let mut announced = false;
    while let Ok(event) = events.try_recv() {
        announced |= matches!(event, Event::NewMail { .. });
    }
    assert!(announced, "NewMail was broadcast");
    // A dropped IDLE connection comes back.
    f.demo.work.drop_idle_connections();
    assert!(f.demo.work.wait_for_idle(1, TIMEOUT).await);
    f.service.shutdown().await;
}

#[tokio::test]
async fn writes_made_offline_replay_after_reconnecting() {
    let f = setup().await;
    synced(&f).await;
    control(&f, "imap-offline", json!({ "offline": true }));
    eventually("work offline", || async {
        statuses(&f.service)
            .await
            .contains(&(AccountId::new(MULTI_WORK_ID), AccountState::Offline))
    })
    .await;
    let message = find(
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
                message_ids: vec![message.id.clone()],
                change: FlagChange {
                    flagged: Some(true),
                    ..FlagChange::default()
                },
            },
        )
        .await,
    );
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

    control(&f, "imap-offline", json!({ "offline": false }));
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
    control(
        &f,
        "pop3-deliver",
        json!({ "raw": "From: grace@home.example\r\nSubject: Fresh from POP3\r\nMessage-ID: <fresh@home.example>\r\n\r\nnew", "uid": "home-3" }),
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
        control(&f, "pop3-state", json!({}))["deleted"]
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
        in_reply_to: None,
    }
}

async fn save(service: &Service, draft: Draft) -> nail_api::DraftId {
    match request(service, Request::SaveDraft { draft }).await {
        Response::DraftSaved { draft_id } => draft_id,
        other => panic!("unexpected {other:?}"),
    }
}

fn smtp_subjects(f: &Fixture) -> Vec<String> {
    control(f, "smtp-messages", json!({}))
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["data"].as_str().unwrap().to_owned())
        .collect()
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
        },
    )
    .await
    else {
        panic!()
    };
    eventually("SMTP received it", || async {
        smtp_subjects(&f)
            .iter()
            .any(|d| d.contains("Sent for real"))
    })
    .await;
    assert!(
        !smtp_subjects(&f)
            .iter()
            .any(|d| d.contains("Cancelled by undo"))
    );
    let data = smtp_subjects(&f).remove(0);
    assert!(
        data.contains("From: Ada Lovelace <ada@work.example>"),
        "{data}"
    );
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
        },
    )
    .await
    else {
        panic!()
    };
    let candidate = candidates.first().expect("autoconfig found the servers");
    assert_eq!(candidate.incoming.server().port, f.demo.new_imap.port());

    let config = AccountConfig {
        id: AccountId::new(""),
        name: "Bob".into(),
        email: MULTI_NEW_EMAIL.into(),
        enabled: true,
        incoming: candidate.incoming.clone(),
        outgoing: candidate.outgoing.clone(),
        identities: vec![],
        sync: Default::default(),
        save_sent_copy: None,
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
        result.outgoing.as_ref().is_some_and(|o| o.ok()),
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
    assert_eq!(f.demo.secrets.len(), 2, "its keychain entry is gone");
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
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    loop {
        let event = tokio::time::timeout_at(deadline, events.recv())
            .await
            .expect("an accounts.kdl error")
            .unwrap();
        if let Event::ConfigError { file, .. } = event {
            assert_eq!(file, "accounts.kdl");
            break;
        }
    }
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
        },
    )
    .await;
    let retry = send_outcome(&mut events).await;
    assert_eq!(retry, (OutboxState::Scheduled, Some(ErrorCode::Network)));
    let sent = send_outcome(&mut events).await;
    assert_eq!(sent, (OutboxState::Sent, None));
    assert!(smtp_subjects(&f).iter().any(|d| d.contains("Second try")));

    // Drafts need a known identity; rich content waits for M1b.
    let mut bad = draft("nobody@nowhere.example", "bob@example.org", "x");
    assert!(matches!(
        request(&f.service, Request::SaveDraft { draft: bad.clone() }).await,
        Response::Error {
            code: ErrorCode::InvalidRequest,
            ..
        }
    ));
    bad.identity_id = MULTI_WORK_EMAIL.into();
    bad.html = Some("<p>x</p>".into());
    assert!(matches!(
        request(&f.service, Request::SaveDraft { draft: bad }).await,
        Response::Error {
            code: ErrorCode::Unsupported,
            ..
        }
    ));
    f.service.shutdown().await;
}

/// The next final (non-`sending`) SendStatus.
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
    assert!(smtp_subjects(&f).is_empty());
    f.service.shutdown().await;
}
