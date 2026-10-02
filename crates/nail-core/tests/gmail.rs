//! Gmail accounts end to end through the core (M3b WP85, ADR 0070): the
//! Gmail API engine against the mock Gmail server (OAuth2 tokens from the
//! mock authorization server): full sync with folders from labels, user
//! labels as tags, categories (`category:` search), flag / tag / move
//! replay, sending through the API, server-side changes picked up by
//! history polling, history expiry, the label queue, `TestAccount`.

use std::sync::Arc;
use std::time::Duration;

use nail_api::accounts::{AccountsConfig, SecretKind, SecretRef};
use nail_api::engine::{MemorySecretStore, SecretStore};
use nail_api::gmail::{GmailCategory, GmailLabelKind, GmailLabelSettings};
use nail_api::{
    AccountId, AccountState, Address, Draft, Event, FlagChange, FolderRole, MessageSummary,
    MoveTarget, Request, Response, SearchQuery,
};
use nail_core::{EngineTiming, Service, ServiceOptions};
use nail_proto_gmail::testing::{InjectedError, MockGmailServer};
use nail_testkit_net::{MockOAuthConfig, MockOAuthServer};

const TIMEOUT: Duration = Duration::from_secs(20);
const ACCOUNT: &str = "gm";

struct Env {
    service: Service,
    gmail: MockGmailServer,
    _oauth: MockOAuthServer,
    _dirs: (tempfile::TempDir, tempfile::TempDir),
}

async fn setup() -> Env {
    let gmail = MockGmailServer::start_demo().await.unwrap();
    let oauth = MockOAuthServer::start(MockOAuthConfig::default())
        .await
        .unwrap();
    oauth.add_refresh_token("refresh-gm", "ada@gmail.example", "mail");
    let data = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    std::fs::write(
        config.path().join("accounts.kdl"),
        format!(
            r#"oauth-provider "mockg" client-id="nail-test-client" auth-url="{auth}" token-url="{token}" {{
    scopes "mail"
}}
account "{ACCOUNT}" email="ada@gmail.example" name="Gmail" {{
    gmail host="127.0.0.1" port={port} tls="plaintext" poll-interval=10 max-poll-interval=10 {{
        auth "oauth2" provider="mockg"
    }}
}}
"#,
            auth = oauth.authorize_url(),
            token = oauth.token_url(),
            port = gmail.addr().port(),
        ),
    )
    .unwrap();
    let secrets = Arc::new(MemorySecretStore::new());
    secrets
        .set(
            &SecretRef::default_for(&AccountId::new(ACCOUNT), SecretKind::OAuth2),
            "refresh-gm",
        )
        .unwrap();
    let mut options = ServiceOptions::new(data.path(), config.path());
    options.watch_config = false;
    options.secrets = Some(secrets as Arc<dyn SecretStore>);
    options.timing = EngineTiming::fast();
    let service = Service::open(nail_core::demo::demo_options(options)).unwrap();
    Env {
        service,
        gmail,
        _oauth: oauth,
        _dirs: (data, config),
    }
}

async fn request(service: &Service, request: Request) -> Response {
    let response = service.handle(request).await;
    if let Response::Error { message, .. } = &response {
        panic!("request failed: {message}");
    }
    response
}

async fn folder(service: &Service, role: FolderRole) -> nail_api::Folder {
    match request(
        service,
        Request::ListFolders {
            account_id: AccountId::new(ACCOUNT),
        },
    )
    .await
    {
        Response::Folders { folders, .. } => folders
            .into_iter()
            .find(|f| f.role == role)
            .expect("folder"),
        other => panic!("unexpected {other:?}"),
    }
}

async fn messages(service: &Service, role: FolderRole) -> Vec<MessageSummary> {
    let folder = folder(service, role).await;
    match request(
        service,
        Request::ListMessages {
            folder_id: folder.id,
            offset: 0,
            limit: 100,
        },
    )
    .await
    {
        Response::Messages { messages, .. } => messages,
        other => panic!("unexpected {other:?}"),
    }
}

/// Waits until `check` holds (polling the core).
async fn until<F, Fut>(what: &str, mut check: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    loop {
        if check().await {
            return;
        }
        assert!(tokio::time::Instant::now() < deadline, "timed out: {what}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn synced(env: &Env) {
    until("the first Gmail sync", || async {
        messages(&env.service, FolderRole::Inbox).await.len() >= 4
    })
    .await;
}

fn by_subject<'a>(list: &'a [MessageSummary], subject: &str) -> &'a MessageSummary {
    list.iter()
        .find(|m| m.subject.contains(subject))
        .unwrap_or_else(|| panic!("no message {subject:?} in {list:?}"))
}

#[tokio::test(flavor = "multi_thread")]
async fn gmail_syncs_folders_labels_and_categories() {
    let env = setup().await;
    synced(&env).await;
    let statuses = env.service.handle(Request::ListAccountStatuses).await;
    let Response::AccountStatuses { statuses } = statuses else {
        panic!("{statuses:?}");
    };
    assert_eq!(statuses[0].state, AccountState::Online);

    let inbox = messages(&env.service, FolderRole::Inbox).await;
    let lunch = by_subject(&inbox, "Lunch on Friday");
    assert!(!lunch.flags.seen);
    let gmail = lunch.gmail.as_ref().expect("Gmail data on the summary");
    assert_eq!(gmail.gmail_id, "18c0000000000001");
    // Sent, archive (a starred message with a user label = tag).
    let sent = messages(&env.service, FolderRole::Sent).await;
    assert!(sent.iter().any(|m| m.subject.contains("Re: Lunch")));
    let archive = messages(&env.service, FolderRole::Archive).await;
    let notes = by_subject(&archive, "Analytical engine");
    assert!(notes.flags.flagged);
    assert_eq!(notes.tags, ["Work/Projects"]);
    assert_eq!(
        notes.gmail.as_ref().unwrap().category,
        Some(GmailCategory::Updates)
    );

    // Labels with counts; user labels carry their tag.
    let Response::GmailLabels { labels, .. } = request(
        &env.service,
        Request::ListGmailLabels {
            account_id: AccountId::new(ACCOUNT),
        },
    )
    .await
    else {
        panic!()
    };
    let work = labels
        .iter()
        .find(|l| l.name == "Work/Projects")
        .expect("user label");
    assert_eq!(work.kind, GmailLabelKind::User);
    assert_eq!(work.tag.as_deref(), Some("Work/Projects"));
    assert_eq!(work.total, 1);
    assert!(labels.iter().any(|l| l.kind == GmailLabelKind::System));

    // Categories are a search field (local index, M3b).
    let Response::SearchResults { hits, .. } = request(
        &env.service,
        Request::Search {
            query: SearchQuery {
                text: "category:social".into(),
                account_id: Some(AccountId::new(ACCOUNT)),
                folder_id: None,
                offset: 0,
                limit: 50,
                sort: Default::default(),
            },
        },
    )
    .await
    else {
        panic!()
    };
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert!(hits[0].message.subject.contains("Charles mentioned you"));
}

#[tokio::test(flavor = "multi_thread")]
async fn gmail_writes_replay_as_label_changes_and_mail_is_sent_through_the_api() {
    let env = setup().await;
    synced(&env).await;
    let inbox = messages(&env.service, FolderRole::Inbox).await;
    let lunch = by_subject(&inbox, "Lunch on Friday").clone();
    let sale = by_subject(&inbox, "Autumn sale").clone();

    // Read + a new tag (creates the label on the server).
    request(
        &env.service,
        Request::UpdateFlags {
            message_ids: vec![lunch.id.clone()],
            change: FlagChange {
                seen: Some(true),
                add_tags: vec!["Friends".into()],
                ..FlagChange::default()
            },
        },
    )
    .await;
    until("the read flag and the new label reach Gmail", || async {
        let labels = env.gmail.labels_of("18c0000000000001");
        let has_friends = env
            .gmail
            .labels()
            .iter()
            .any(|l| l.name == "Friends" && labels.contains(&l.id));
        !labels.iter().any(|l| l == "UNREAD") && has_friends
    })
    .await;

    // Archive: INBOX goes.
    request(
        &env.service,
        Request::MoveMessages {
            message_ids: vec![sale.id.clone()],
            target: MoveTarget::Role(FolderRole::Archive),
        },
    )
    .await;
    until("the archive reaches Gmail", || async {
        !env.gmail
            .labels_of("18c0000000000003")
            .iter()
            .any(|l| l == "INBOX")
    })
    .await;
    // The next history poll keeps it archived (stored once, moved).
    env.service
        .handle(Request::SyncNow {
            account_id: Some(AccountId::new(ACCOUNT)),
        })
        .await;
    until("the archived message stays in the archive", || async {
        let archive = messages(&env.service, FolderRole::Archive).await;
        archive.iter().any(|m| m.id == sale.id)
    })
    .await;
    assert!(
        !messages(&env.service, FolderRole::Inbox)
            .await
            .iter()
            .any(|m| m.id == sale.id)
    );

    // Sending goes through messages.send (no SMTP server configured).
    let mut draft = Draft::new("ada@gmail.example");
    draft.to = vec![Address {
        name: None,
        email: "grace@example.org".into(),
    }];
    draft.subject = "Sent through the Gmail API".into();
    draft.text = "Hello".into();
    let Response::DraftSaved { draft_id } =
        request(&env.service, Request::SaveDraft { draft }).await
    else {
        panic!()
    };
    request(
        &env.service,
        Request::SendDraft {
            draft_id,
            delay_secs: Some(0),
            send_at: None,
            crypto: None,
            expected_revision: None,
        },
    )
    .await;
    until("Gmail received the sent message", || async {
        env.gmail
            .sent()
            .iter()
            .any(|raw| String::from_utf8_lossy(raw).contains("Sent through the Gmail API"))
    })
    .await;
    // Gmail files it in Sent; the next poll brings it (no local copy).
    env.service
        .handle(Request::SyncNow {
            account_id: Some(AccountId::new(ACCOUNT)),
        })
        .await;
    until("the sent message shows in Sent once", || async {
        messages(&env.service, FolderRole::Sent)
            .await
            .iter()
            .filter(|m| m.subject == "Sent through the Gmail API")
            .count()
            == 1
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn server_side_changes_arrive_by_history_and_expiry_resyncs() {
    let env = setup().await;
    synced(&env).await;
    let mut events = env.service.subscribe();
    let id = env.gmail.deliver(
        b"From: Grace Hopper <grace@example.org>\r\nTo: ada@gmail.example\r\nSubject: Compiler news\r\nDate: Thu, 01 Oct 2026 12:00:00 +0000\r\nMessage-ID: <compiler@example.org>\r\n\r\nA-0 works.\r\n",
        &["INBOX", "UNREAD", "CATEGORY_PERSONAL"],
    );
    env.service
        .handle(Request::SyncNow {
            account_id: Some(AccountId::new(ACCOUNT)),
        })
        .await;
    until("the new message arrives", || async {
        messages(&env.service, FolderRole::Inbox)
            .await
            .iter()
            .any(|m| m.subject == "Compiler news")
    })
    .await;
    // Announced as new mail (rules, notifications).
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    loop {
        let event = tokio::time::timeout_at(deadline, events.recv())
            .await
            .expect("NewMail in time")
            .unwrap();
        if let Event::NewMail { message_ids, .. } = event {
            assert_eq!(message_ids.len(), 1);
            break;
        }
    }
    // A label change on the server (starred, read).
    env.gmail.modify(&id, &["STARRED"], &["UNREAD"]);
    env.service
        .handle(Request::SyncNow {
            account_id: Some(AccountId::new(ACCOUNT)),
        })
        .await;
    until("the server's flags arrive", || async {
        messages(&env.service, FolderRole::Inbox)
            .await
            .iter()
            .any(|m| m.subject == "Compiler news" && m.flags.flagged && m.flags.seen)
    })
    .await;
    // Deleted on the server, and the history expires: a full resync
    // merges (the deleted message goes, the rest stays).
    env.gmail.delete(&id);
    env.gmail.expire_history();
    env.service
        .handle(Request::SyncNow {
            account_id: Some(AccountId::new(ACCOUNT)),
        })
        .await;
    until("the resync removes the deleted message", || async {
        let inbox = messages(&env.service, FolderRole::Inbox).await;
        !inbox.iter().any(|m| m.subject == "Compiler news")
            && inbox.iter().any(|m| m.subject.contains("Lunch on Friday"))
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn labels_are_created_renamed_and_deleted_through_the_queue() {
    let env = setup().await;
    synced(&env).await;
    let account = AccountId::new(ACCOUNT);
    let Response::GmailLabelSaved { label, op_ids } = request(
        &env.service,
        Request::CreateGmailLabel {
            account_id: account.clone(),
            settings: GmailLabelSettings::named("Travel"),
        },
    )
    .await
    else {
        panic!()
    };
    assert!(label.id.as_str().starts_with("local-"));
    assert_eq!(op_ids.len(), 1);
    until("the label is created on the server", || async {
        env.gmail.labels().iter().any(|l| l.name == "Travel")
    })
    .await;
    // Duplicates and invalid names are refused with their texts.
    let refused = env
        .service
        .handle(Request::CreateGmailLabel {
            account_id: account.clone(),
            settings: GmailLabelSettings::named("travel"),
        })
        .await;
    let Response::Error { localized, .. } = refused else {
        panic!("{refused:?}")
    };
    assert_eq!(
        localized.as_ref().map(|l| l.id.as_str()),
        Some("gmail-label-issue-duplicate-name")
    );

    // The server id replaced the local one; rename it.
    let server_id = until_label(&env, "Travel").await;
    request(
        &env.service,
        Request::UpdateGmailLabel {
            account_id: account.clone(),
            label_id: server_id.clone(),
            settings: GmailLabelSettings::named("Trips"),
        },
    )
    .await;
    until("the rename reaches the server", || async {
        env.gmail.labels().iter().any(|l| l.name == "Trips")
    })
    .await;
    request(
        &env.service,
        Request::DeleteGmailLabel {
            account_id: account.clone(),
            label_id: server_id,
        },
    )
    .await;
    until("the label is deleted on the server", || async {
        !env.gmail.labels().iter().any(|l| l.name == "Trips")
    })
    .await;
    // Other accounts are refused.
    let other = env
        .service
        .handle(Request::ListGmailLabels {
            account_id: AccountId::new("nope"),
        })
        .await;
    assert!(matches!(other, Response::Error { .. }));
}

async fn until_label(env: &Env, name: &str) -> nail_api::GmailLabelId {
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    loop {
        if let Response::GmailLabels { labels, .. } = env
            .service
            .handle(Request::ListGmailLabels {
                account_id: AccountId::new(ACCOUNT),
            })
            .await
            && let Some(label) = labels
                .iter()
                .find(|l| l.name == name && !l.id.as_str().starts_with("local-"))
        {
            return label.id.clone();
        }
        assert!(tokio::time::Instant::now() < deadline, "label {name}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn test_account_reads_the_gmail_profile() {
    let env = setup().await;
    let config: AccountsConfig = match env.service.handle(Request::GetAccountsConfig).await {
        Response::AccountsConfig { config } => config,
        other => panic!("{other:?}"),
    };
    let account = config.accounts[0].clone();
    let Response::AccountTested { result } = env
        .service
        .handle(Request::TestAccount {
            config: Box::new(account),
            secrets: Vec::new(),
        })
        .await
    else {
        panic!()
    };
    assert_eq!(result.incoming.error, None, "{:?}", result.incoming);
    assert_eq!(result.incoming.capabilities, ["gmail:ada@gmail.example"]);
    assert!(result.outgoing.is_none());
}

/// An iMIP invitation from Grace to the Gmail user.
fn gmail_invitation(uid: &str, subject: &str) -> Vec<u8> {
    format!(
        "From: Grace Hopper <grace@example.org>\r\nTo: ada@gmail.example\r\nSubject: {subject}\r\n\
         Date: Thu, 01 Oct 2026 12:00:00 +0000\r\nMessage-ID: <{uid}>\r\nMIME-Version: 1.0\r\n\
         Content-Type: text/calendar; method=REQUEST; charset=utf-8\r\n\r\n\
         BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Test//EN\r\nMETHOD:REQUEST\r\nBEGIN:VEVENT\r\n\
         UID:{uid}\r\nDTSTAMP:20261001T080000Z\r\nSEQUENCE:0\r\nDTSTART:20261005T090000Z\r\n\
         DTEND:20261005T100000Z\r\nSUMMARY:{subject}\r\nORGANIZER:mailto:grace@example.org\r\n\
         ATTENDEE;RSVP=TRUE:mailto:ada@gmail.example\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
    )
    .into_bytes()
}

async fn invitation_event(env: &Env, message: &MessageSummary) -> Option<nail_api::EventId> {
    match request(
        &env.service,
        Request::GetInvitation {
            message_id: message.id.clone(),
        },
    )
    .await
    {
        Response::Invitation {
            invitation: Some(card),
        } => card.event_id,
        other => panic!("no card: {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn only_inbox_mail_counts_as_arriving() {
    // Review M3b: new Gmail mail in Sent, Spam or Trash ran through the
    // arrival rules and iTIP processing.
    let env = setup().await;
    synced(&env).await;
    env.gmail.deliver(
        &gmail_invitation("sent-1@example.org", "Copy in Sent"),
        &["SENT"],
    );
    env.gmail.deliver(
        &gmail_invitation("trash-1@example.org", "Copy in Trash"),
        &["TRASH"],
    );
    env.gmail.deliver(
        &gmail_invitation("inbox-1@example.org", "Invitation in the inbox"),
        &["INBOX", "UNREAD"],
    );
    env.service
        .handle(Request::SyncNow {
            account_id: Some(AccountId::new(ACCOUNT)),
        })
        .await;
    until("the inbox invitation is added to the calendar", || async {
        let inbox = messages(&env.service, FolderRole::Inbox).await;
        match inbox
            .iter()
            .find(|m| m.subject == "Invitation in the inbox")
        {
            Some(m) => invitation_event(&env, m).await.is_some(),
            None => false,
        }
    })
    .await;
    for (role, subject) in [
        (FolderRole::Sent, "Copy in Sent"),
        (FolderRole::Trash, "Copy in Trash"),
    ] {
        let list = messages(&env.service, role).await;
        let message = by_subject(&list, subject);
        assert_eq!(invitation_event(&env, message).await, None, "{subject}");
    }
}

fn plain(message_id: &str, subject: &str) -> Vec<u8> {
    format!(
        "From: Grace Hopper <grace@example.org>\r\nTo: ada@gmail.example\r\nSubject: {subject}\r\n\
         Date: Thu, 01 Oct 2026 12:00:00 +0000\r\nMessage-ID: <{message_id}>\r\n\r\nHello.\r\n"
    )
    .into_bytes()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_resumed_full_listing_still_removes_deleted_messages() {
    // Review M3b: after a history expiry, a full listing interrupted and
    // resumed on the next pass skipped the merge, so messages deleted on
    // the server stayed in Nail.
    let env = setup().await;
    synced(&env).await;
    let doomed = env
        .gmail
        .deliver(&plain("doomed@example.org", "Doomed"), &["INBOX"]);
    env.service
        .handle(Request::SyncNow {
            account_id: Some(AccountId::new(ACCOUNT)),
        })
        .await;
    until("the message arrives", || async {
        messages(&env.service, FolderRole::Inbox)
            .await
            .iter()
            .any(|m| m.subject == "Doomed")
    })
    .await;
    // More than one listing page (100 messages a page).
    for i in 0..105 {
        env.gmail
            .deliver(&plain(&format!("filler-{i}@example.org"), "Filler"), &[]);
    }
    env.gmail.delete(&doomed);
    env.gmail.expire_history();
    // The second page fails once: the pass ends, the next one resumes.
    env.gmail.clear_requests();
    env.gmail
        .inject(InjectedError::status(400).on("pageToken").method("GET"));
    env.service
        .handle(Request::SyncNow {
            account_id: Some(AccountId::new(ACCOUNT)),
        })
        .await;
    until(
        "the resumed listing removes the deleted message",
        || async {
            let resumed = env
                .gmail
                .requests()
                .iter()
                .filter(|r| r.path.ends_with("/messages") && r.query.contains("pageToken"))
                .count()
                >= 2;
            resumed
                && !messages(&env.service, FolderRole::Inbox)
                    .await
                    .iter()
                    .any(|m| m.subject == "Doomed")
        },
    )
    .await;
    // Nothing else went.
    let inbox = messages(&env.service, FolderRole::Inbox).await;
    assert!(inbox.iter().any(|m| m.subject.contains("Lunch on Friday")));
    let archive = messages(&env.service, FolderRole::Archive).await;
    assert!(archive.iter().filter(|m| m.subject == "Filler").count() >= 100);
}
