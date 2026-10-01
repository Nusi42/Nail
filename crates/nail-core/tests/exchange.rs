//! Exchange accounts end to end through the core (M3c WP95, ADR 0077):
//! the Graph engine against the mock Graph server (OAuth2 tokens from the
//! mock authorization server) and the EWS engine against the mock EWS
//! server (Basic): mail per folder with Focused Inbox and categories as
//! tags, replay of flags / moves / tags, `focus:` search, the Exchange
//! queue (categories, Focused Inbox, automatic replies, server rules),
//! shared mailboxes, rooms and free/busy, sending through the engine,
//! calendars / contacts / To Do, meetings through the meeting API,
//! placement of Nail rules as inbox rules, EWS streaming, `TestAccount`.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use nail_api::accounts::{SecretKind, SecretRef};
use nail_api::engine::{MemorySecretStore, SecretStore};
use nail_api::exchange::oof::{AutomaticRepliesStatus, ExternalAudience};
use nail_api::exchange::{
    AutomaticRepliesSettings, CategoryColor, ExchangeCategorySettings, ExchangeMailboxSettings,
    ExchangeMailboxState, InferenceClassification,
};
use nail_api::{
    AccountId, AccountState, Address, Draft, FlagChange, FolderRole, MessageSummary, MoveTarget,
    Request, Response, SearchQuery,
};
use nail_core::{EngineTiming, Service, ServiceOptions};
use nail_proto_ews::testing::{MockAuth, MockEwsServer};
use nail_proto_graph::testing::MockGraphServer;
use nail_testkit_net::{MockOAuthConfig, MockOAuthServer};

const TIMEOUT: Duration = Duration::from_secs(30);
const GRAPH: &str = "contoso";
const EWS: &str = "corp";

struct Env {
    service: Service,
    graph: MockGraphServer,
    ews: Option<MockEwsServer>,
    _oauth: MockOAuthServer,
    _dirs: (tempfile::TempDir, tempfile::TempDir),
}

async fn setup(with_ews: bool, extra: &str) -> Env {
    let graph = MockGraphServer::start_demo().await.unwrap();
    let oauth = MockOAuthServer::start(MockOAuthConfig::default())
        .await
        .unwrap();
    oauth.add_refresh_token("refresh-ex", "ada@contoso.example", "graph");
    let ews = if with_ews {
        Some(
            MockEwsServer::start_with(MockAuth::Basic {
                username: "ada@corp.example".into(),
                password: "ews-secret".into(),
            })
            .await
            .unwrap(),
        )
    } else {
        None
    };
    let data = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let ews_node = ews.as_ref().map_or_else(String::new, |ews| {
        format!(
            r#"account "{EWS}" email="ada@corp.example" name="Corp" {{
    exchange engine="ews" host="127.0.0.1" url="{url}" tls="plaintext" poll-interval=10 max-poll-interval=10 {{
        auth "password" username="ada@corp.example"
    }}
}}
"#,
            url = ews.url()
        )
    });
    std::fs::write(
        config.path().join("accounts.kdl"),
        format!(
            r#"oauth-provider "mockms" client-id="nail-test-client" auth-url="{auth}" token-url="{token}" {{
    scopes "graph"
}}
account "{GRAPH}" email="ada@contoso.example" name="Contoso" {{
    exchange host="127.0.0.1" port={port} tls="plaintext" poll-interval=10 max-poll-interval=10 {{
        auth "oauth2" provider="mockms"
    }}
    identity "ada@contoso.example" name="Ada Lovelace"
}}
{ews_node}{extra}"#,
            auth = oauth.authorize_url(),
            token = oauth.token_url(),
            port = graph.addr().port(),
        ),
    )
    .unwrap();
    let secrets = Arc::new(MemorySecretStore::new());
    secrets
        .set(
            &SecretRef::default_for(&AccountId::new(GRAPH), SecretKind::OAuth2),
            "refresh-ex",
        )
        .unwrap();
    secrets
        .set(
            &SecretRef::default_for(&AccountId::new(EWS), SecretKind::Password),
            "ews-secret",
        )
        .unwrap();
    let mut options = ServiceOptions::new(data.path(), config.path());
    options.watch_config = false;
    options.secrets = Some(secrets as Arc<dyn SecretStore>);
    options.timing = EngineTiming::fast();
    let service = Service::open(nail_core::demo::demo_options(options)).unwrap();
    Env {
        service,
        graph,
        ews,
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

async fn folders(service: &Service, account: &str) -> Vec<nail_api::Folder> {
    match request(
        service,
        Request::ListFolders {
            account_id: AccountId::new(account),
        },
    )
    .await
    {
        Response::Folders { folders, .. } => folders,
        other => panic!("unexpected {other:?}"),
    }
}

async fn folder_at(service: &Service, account: &str, path: &str) -> Option<nail_api::Folder> {
    folders(service, account)
        .await
        .into_iter()
        .find(|f| f.path == path)
}

async fn messages_in(service: &Service, folder: &nail_api::Folder) -> Vec<MessageSummary> {
    match request(
        service,
        Request::ListMessages {
            folder_id: folder.id.clone(),
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

async fn inbox(service: &Service, account: &str) -> Vec<MessageSummary> {
    let Some(folder) = folders(service, account)
        .await
        .into_iter()
        .find(|f| f.role == FolderRole::Inbox)
    else {
        return Vec::new();
    };
    messages_in(service, &folder).await
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

fn subject_of<'a>(list: &'a [MessageSummary], subject: &str) -> &'a MessageSummary {
    list.iter()
        .find(|m| m.subject == subject)
        .unwrap_or_else(|| panic!("no message {subject:?}"))
}

#[tokio::test(flavor = "multi_thread")]
async fn a_graph_account_syncs_mail_with_focused_inbox_and_categories() {
    let env = setup(false, "").await;
    let service = &env.service;
    until("the Graph inbox", || async {
        inbox(service, GRAPH).await.len() == 2
    })
    .await;
    let list = inbox(service, GRAPH).await;
    let review = subject_of(&list, "Engine review on Monday");
    assert!(review.flags.flagged && !review.flags.seen);
    assert_eq!(review.tags, ["Red category"]);
    let info = review.exchange.as_ref().expect("Exchange data");
    assert_eq!(info.inference, Some(InferenceClassification::Focused));
    let news = subject_of(&list, "Fabrikam newsletter, October");
    assert_eq!(
        news.exchange.as_ref().unwrap().inference,
        Some(InferenceClassification::Other)
    );
    // The folder tree (paths from the parents) and roles.
    until("the subfolders", || async {
        folder_at(service, GRAPH, "Inbox/Receipts").await.is_some()
            && folder_at(service, GRAPH, "Projects/Engine").await.is_some()
    })
    .await;
    let sent = folders(service, GRAPH)
        .await
        .into_iter()
        .find(|f| f.role == FolderRole::Sent)
        .expect("Sent Items");
    assert_eq!(messages_in(service, &sent).await.len(), 1);
    // `focus:` searches.
    let Response::SearchResults { hits, .. } = request(
        service,
        Request::Search {
            query: SearchQuery {
                text: "focus:other".into(),
                account_id: None,
                folder_id: None,
                offset: 0,
                limit: 50,
                sort: Default::default(),
            },
        },
    )
    .await
    else {
        panic!("unexpected answer")
    };
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].message.subject, "Fabrikam newsletter, October");
    // Status.
    let Response::ExchangeStatus { status } = request(
        service,
        Request::GetExchangeStatus {
            account_id: AccountId::new(GRAPH),
        },
    )
    .await
    else {
        panic!()
    };
    assert_eq!(status.engine, nail_api::exchange::ExchangeEngine::Graph);
    assert!(status.folders_total >= 5, "{status:?}");
    assert_eq!(status.push, nail_api::exchange::ExchangePush::Polling);
}

#[tokio::test(flavor = "multi_thread")]
async fn writes_replay_against_graph() {
    let env = setup(false, "").await;
    let service = &env.service;
    until("the Graph inbox", || async {
        inbox(service, GRAPH).await.len() == 2
    })
    .await;
    let list = inbox(service, GRAPH).await;
    let news = subject_of(&list, "Fabrikam newsletter, October").clone();
    let review = subject_of(&list, "Engine review on Monday").clone();
    // Flag + a tag (= a category).
    request(
        service,
        Request::UpdateFlags {
            message_ids: vec![news.id.clone()],
            change: FlagChange {
                flagged: Some(true),
                add_tags: vec!["Partner".into()],
                ..FlagChange::default()
            },
        },
    )
    .await;
    let inbox_id = env.graph.folder_id(None, "inbox");
    until("the flag on the server", || async {
        env.graph.messages_in(None, &inbox_id).iter().any(|m| {
            m.raw.windows(9).any(|w| w == b"Fabrikam ")
                && m.flag == "flagged"
                && m.categories.contains(&"Partner".to_owned())
        })
    })
    .await;
    // Move to Receipts: the server's new id is kept.
    let receipts = folder_at(service, GRAPH, "Inbox/Receipts").await.unwrap();
    request(
        service,
        Request::MoveMessages {
            message_ids: vec![review.id.clone()],
            target: MoveTarget::FolderId(receipts.id.clone()),
        },
    )
    .await;
    let receipts_remote = env.graph.with_mailbox(None, |m, _| {
        m.folders
            .values()
            .find(|f| f.name == "Receipts")
            .map(|f| f.id.clone())
            .unwrap()
    });
    until("the move on the server", || async {
        env.graph
            .messages_in(None, &receipts_remote)
            .iter()
            .any(|m| String::from_utf8_lossy(&m.raw).contains("Engine review on Monday"))
    })
    .await;
    until("the moved message stays put", || async {
        messages_in(service, &receipts)
            .await
            .iter()
            .any(|m| m.subject == "Engine review on Monday")
    })
    .await;
    // Focused Inbox through the Exchange queue.
    request(
        service,
        Request::ClassifyFocused {
            message_ids: vec![news.id.clone()],
            classification: InferenceClassification::Focused,
            always: false,
        },
    )
    .await;
    until("the classification on the server", || async {
        env.graph.messages_in(None, &inbox_id).iter().any(|m| {
            String::from_utf8_lossy(&m.raw).contains("Fabrikam newsletter")
                && m.inference == "focused"
        })
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn categories_automatic_replies_and_rooms() {
    let env = setup(false, "").await;
    let service = &env.service;
    let account = || AccountId::new(GRAPH);
    until("the categories", || async {
        matches!(
            service.handle(Request::ListExchangeCategories { account_id: account() }).await,
            Response::ExchangeCategories { categories, .. } if categories.len() == 3
        )
    })
    .await;
    let Response::ExchangeCategorySaved { category, .. } = request(
        service,
        Request::CreateExchangeCategory {
            account_id: account(),
            settings: ExchangeCategorySettings {
                name: "Blue".into(),
                color: CategoryColor::Preset7,
            },
        },
    )
    .await
    else {
        panic!()
    };
    assert!(category.id.as_str().starts_with("local-"));
    until("the category on the server", || async {
        env.graph.with_mailbox(None, |m, _| {
            m.categories.iter().any(|c| c["displayName"] == "Blue")
        })
    })
    .await;
    // A duplicate name is refused with its text.
    let refused = service
        .handle(Request::CreateExchangeCategory {
            account_id: account(),
            settings: ExchangeCategorySettings::named("blue"),
        })
        .await;
    assert!(matches!(
        refused,
        Response::Error {
            code: nail_api::ErrorCode::InvalidRequest,
            ..
        }
    ));

    // Automatic replies: read from the server, then changed.
    until("the automatic replies", || async {
        matches!(
            service.handle(Request::GetAutomaticReplies { account_id: account() }).await,
            Response::AutomaticReplies { replies, .. } if replies.fetched_at.is_some()
        )
    })
    .await;
    let Response::AutomaticReplies { replies, .. } = request(
        service,
        Request::GetAutomaticReplies {
            account_id: account(),
        },
    )
    .await
    else {
        panic!()
    };
    assert_eq!(replies.settings.status, AutomaticRepliesStatus::Scheduled);
    assert!(replies.settings.internal_reply.contains("conference"));
    request(
        service,
        Request::SetAutomaticReplies {
            account_id: account(),
            settings: AutomaticRepliesSettings {
                status: AutomaticRepliesStatus::AlwaysEnabled,
                start: None,
                end: None,
                internal_reply: "Out until Monday.".into(),
                external_reply: "Out until Monday.".into(),
                external_audience: ExternalAudience::All,
            },
        },
    )
    .await;
    until("the automatic replies on the server", || async {
        env.graph.with_mailbox(None, |m, _| {
            m.automatic_replies["status"] == "alwaysEnabled"
                && m.automatic_replies["internalReplyMessage"]
                    .as_str()
                    .is_some_and(|t| t.contains("Out until Monday."))
        })
    })
    .await;
    // The generic vacation reply shows the same.
    let Response::Vacation { settings, .. } = request(
        service,
        Request::GetVacation {
            account_id: account(),
        },
    )
    .await
    else {
        panic!()
    };
    assert!(settings.enabled && settings.body.contains("Out until Monday."));

    // Rooms with availability.
    let Response::RoomLists { lists, .. } = request(
        service,
        Request::ListRoomLists {
            account_id: account(),
            refresh: true,
        },
    )
    .await
    else {
        panic!()
    };
    assert_eq!(lists.len(), 2);
    let Response::Rooms { rooms, .. } = request(
        service,
        Request::FindRooms {
            account_id: account(),
            room_list: Some("building1@contoso.example".into()),
            from: "2026-10-05T09:00:00Z".into(),
            to: "2026-10-05T10:00:00Z".into(),
            min_capacity: None,
            refresh: true,
        },
    )
    .await
    else {
        panic!()
    };
    assert_eq!(rooms.len(), 2);
    assert_eq!(rooms[0].room.name, "Conf Room Baker");
    assert_eq!(rooms[0].free, Some(true));
    assert_eq!(rooms[1].free, Some(false));
    // Without consent the rooms answer forbidden with the warning text.
    env.graph.forbid_places(true);
    let Response::Error {
        code, localized, ..
    } = service
        .handle(Request::ListRoomLists {
            account_id: account(),
            refresh: true,
        })
        .await
    else {
        panic!("expected an error")
    };
    assert_eq!(code, nail_api::ErrorCode::Forbidden);
    assert_eq!(localized.unwrap().id, "exchange-warning-rooms-need-consent");
    // Non-Exchange accounts are refused.
    let other = service
        .handle(Request::GetExchangeStatus {
            account_id: AccountId::new("nope"),
        })
        .await;
    assert!(matches!(
        other,
        Response::Error {
            code: nail_api::ErrorCode::NotFound,
            ..
        }
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn shared_mailboxes_sending_and_free_busy() {
    let env = setup(false, "").await;
    let service = &env.service;
    let account = || AccountId::new(GRAPH);
    until("the Graph inbox", || async {
        inbox(service, GRAPH).await.len() == 2
    })
    .await;
    let Response::ExchangeMailboxSaved { mailbox } = request(
        service,
        Request::AddExchangeMailbox {
            account_id: account(),
            settings: ExchangeMailboxSettings::shared("support@contoso.example"),
        },
    )
    .await
    else {
        panic!()
    };
    assert_eq!(mailbox.state, ExchangeMailboxState::Pending);
    request(
        service,
        Request::SyncNow {
            account_id: Some(account()),
        },
    )
    .await;
    until("the shared mailbox", || async {
        match folder_at(service, GRAPH, "@support@contoso.example/Inbox").await {
            Some(f) => messages_in(service, &f)
                .await
                .iter()
                .any(|m| m.subject == "Ticket 4711: login fails"),
            None => false,
        }
    })
    .await;
    let Response::ExchangeMailboxes { mailboxes, .. } = request(
        service,
        Request::ListExchangeMailboxes {
            account_id: account(),
        },
    )
    .await
    else {
        panic!()
    };
    assert_eq!(mailboxes[0].state, ExchangeMailboxState::Synced);
    assert!(mailboxes[0].root_folder_id.is_some());
    // A mailbox the user may not open.
    request(
        service,
        Request::AddExchangeMailbox {
            account_id: account(),
            settings: ExchangeMailboxSettings::shared("ceo@contoso.example"),
        },
    )
    .await;
    request(
        service,
        Request::SyncNow {
            account_id: Some(account()),
        },
    )
    .await;
    until("the denied mailbox", || async {
        matches!(
            service.handle(Request::ListExchangeMailboxes { account_id: account() }).await,
            Response::ExchangeMailboxes { mailboxes, .. }
                if mailboxes.iter().any(|m| m.settings.address == "ceo@contoso.example" && m.state == ExchangeMailboxState::AccessDenied)
        )
    })
    .await;
    // Removing a mailbox removes its folders.
    request(
        service,
        Request::RemoveExchangeMailbox {
            account_id: account(),
            address: "support@contoso.example".into(),
        },
    )
    .await;
    assert!(
        folder_at(service, GRAPH, "@support@contoso.example/Inbox")
            .await
            .is_none()
    );

    // Sending goes through Graph.
    let mut draft = Draft::new("ada@contoso.example");
    draft.to = vec![Address {
        name: None,
        email: "bob@contoso.example".into(),
    }];
    draft.subject = "Sent with Graph".into();
    draft.text = "Hello Bob".into();
    let Response::DraftSaved { draft_id } = request(service, Request::SaveDraft { draft }).await
    else {
        panic!()
    };
    request(
        service,
        Request::SendDraft {
            draft_id,
            delay_secs: Some(0),
            send_at: None,
            crypto: None,
        },
    )
    .await;
    until("the mail on Graph", || async {
        env.graph
            .sent()
            .iter()
            .any(|s| String::from_utf8_lossy(&s.raw).contains("Sent with Graph"))
    })
    .await;

    // Free/busy of a colleague through Graph.
    let Response::FreeBusy { result } = request(
        service,
        Request::QueryFreeBusy {
            attendees: vec!["bob@contoso.example".into()],
            from: "2026-10-05T00:00:00Z".into(),
            to: "2026-10-06T00:00:00Z".into(),
            identity_id: None,
            ignore_event_id: None,
        },
    )
    .await
    else {
        panic!()
    };
    assert_eq!(result.attendees.len(), 1);
    assert!(
        matches!(
            result.attendees[0].source,
            nail_api::scheduling::FreeBusySource::Exchange { .. }
        ),
        "{:?}",
        result.attendees[0]
    );
    assert_eq!(result.attendees[0].periods.len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn calendars_contacts_and_to_do() {
    let env = setup(false, "").await;
    let service = &env.service;
    let source = nail_api::exchange::exchange_pim_source_id(&AccountId::new(GRAPH));
    let calendars = || async {
        match service
            .handle(Request::ListCalendars { source_id: None })
            .await
        {
            Response::Calendars { calendars } => calendars
                .into_iter()
                .filter(|c| c.source_id == source)
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        }
    };
    until("the Exchange calendars and To Do lists", || async {
        calendars().await.len() >= 4
    })
    .await;
    let all = calendars().await;
    let calendar = all
        .iter()
        .find(|c| c.settings.name == "Calendar")
        .expect("the default calendar")
        .clone();
    assert!(calendar.exchange.is_some());
    assert_eq!(calendar.account_id, Some(AccountId::new(GRAPH)));
    let todo = all
        .iter()
        .find(|c| c.settings.name == "Tasks")
        .expect("the To Do list")
        .clone();
    assert!(todo.exchange.as_ref().unwrap().todo);
    until("the events", || async {
        matches!(
            service.handle(Request::ListEventInstances {
                calendar_ids: Some(vec![calendar.id.clone()]),
                from: "2026-10-01T00:00:00Z".into(),
                to: "2026-10-31T00:00:00Z".into(),
                tz: "Europe/Berlin".into(),
            }).await,
            Response::EventInstances { instances, .. } if instances.iter().any(|i| i.title == "Dentist")
        )
    })
    .await;
    // To Do with its checklist as subtasks.
    until("the To Do tasks", || async {
        match service
            .handle(Request::ListTasks {
                calendar_ids: Some(vec![todo.id.clone()]),
                include_completed: true,
            })
            .await
        {
            Response::Tasks { tasks } => tasks.iter().any(|t| {
                t.fields.title == "Write the quarterly report" && t.subtask_ids.len() == 2
            }),
            _ => false,
        }
    })
    .await;
    // A new event is created on the server.
    let fields = nail_api::pim::EventFields::new(
        "Planning",
        nail_api::pim::PimTime::zoned("2026-10-12T10:00:00", "Europe/Berlin"),
        nail_api::pim::PimTime::zoned("2026-10-12T11:00:00", "Europe/Berlin"),
    );
    request(
        service,
        Request::CreateEvent {
            calendar_id: calendar.id.clone(),
            fields: Box::new(fields),
            recurrence: None,
        },
    )
    .await;
    until("the event on Graph", || async {
        env.graph
            .events()
            .iter()
            .any(|(_, e)| e["subject"] == "Planning")
    })
    .await;
    // Contacts.
    until("the contacts", || async {
        match service
            .handle(Request::ListContacts {
                address_book_ids: None,
                group_id: None,
                offset: 0,
                limit: 200,
            })
            .await
        {
            Response::Contacts { contacts, .. } => {
                contacts.iter().any(|c| c.display_name == "Adele Vance")
            }
            _ => false,
        }
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_ews_account_syncs_streams_and_tests() {
    let env = setup(true, "").await;
    let service = &env.service;
    until("the EWS inbox", || async {
        inbox(service, EWS).await.len() >= 3
    })
    .await;
    let Response::ExchangeStatus { status } = request(
        service,
        Request::GetExchangeStatus {
            account_id: AccountId::new(EWS),
        },
    )
    .await
    else {
        panic!()
    };
    assert_eq!(status.engine, nail_api::exchange::ExchangeEngine::Ews);
    assert!(status.server_version.is_some());
    assert!(
        status
            .warnings
            .contains(&nail_api::exchange::ExchangeWarning::FocusedInboxUnavailable)
    );
    // A message delivered on the server arrives through streaming.
    let ews = env.ews.as_ref().unwrap();
    let inbox_id = ews.folder_id(None, "inbox").unwrap();
    ews.deliver(
        &inbox_id,
        b"From: Grace <grace@corp.example>\r\nTo: ada@corp.example\r\nSubject: Streaming works\r\nMessage-ID: <stream@corp.example>\r\n\r\nHi\r\n",
        false,
    );
    until("the streamed message", || async {
        inbox(service, EWS)
            .await
            .iter()
            .any(|m| m.subject == "Streaming works")
    })
    .await;
    // Flags replay on EWS.
    let message = inbox(service, EWS)
        .await
        .into_iter()
        .find(|m| m.subject == "Streaming works")
        .unwrap();
    request(
        service,
        Request::UpdateFlags {
            message_ids: vec![message.id.clone()],
            change: FlagChange {
                seen: Some(true),
                ..FlagChange::default()
            },
        },
    )
    .await;
    until("the read flag on EWS", || async {
        ews.items_in(&inbox_id).iter().any(|i| {
            i.text("Subject") == Some("Streaming works") && i.text("IsRead") == Some("true")
        })
    })
    .await;
    // TestAccount with the stored password.
    let config = match request(service, Request::GetAccountsConfig).await {
        Response::AccountsConfig { config } => {
            config.account(&AccountId::new(EWS)).cloned().unwrap()
        }
        other => panic!("unexpected {other:?}"),
    };
    let Response::AccountTested { result } = request(
        service,
        Request::TestAccount {
            config: Box::new(config),
            secrets: Vec::new(),
        },
    )
    .await
    else {
        panic!()
    };
    assert_eq!(result.incoming.error, None, "{result:?}");
    assert!(
        result
            .incoming
            .capabilities
            .contains(&"exchange:ews".to_owned())
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn engines_report_online() {
    let env = setup(false, "").await;
    until("online", || async {
        match env.service.handle(Request::ListAccountStatuses).await {
            Response::AccountStatuses { statuses } => statuses
                .iter()
                .any(|s| s.account_id.as_str() == GRAPH && s.state == AccountState::Online),
            _ => false,
        }
    })
    .await;
}

fn receipts_rule() -> nail_api::rules::Rule {
    use nail_api::rules::{Condition, FolderRef, Rule, RuleAction, RuleTrigger, TextMatch, TextOp};
    Rule {
        id: "".into(),
        name: "Receipts".into(),
        enabled: true,
        triggers: vec![RuleTrigger::Arrival],
        scope: vec![nail_api::rules::RuleScope::Account {
            account_id: AccountId::new(GRAPH),
        }],
        conditions: Condition::All {
            conditions: vec![Condition::Subject {
                matches: TextMatch {
                    op: TextOp::Contains,
                    value: "receipt".into(),
                    case_sensitive: false,
                },
            }],
        },
        actions: vec![RuleAction::Move {
            folder: FolderRef::Path {
                path: "Inbox/Receipts".into(),
                account_id: None,
            },
        }],
        source: None,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn nail_rules_are_placed_as_inbox_rules() {
    let env = setup(false, "").await;
    let service = &env.service;
    let account = || AccountId::new(GRAPH);
    until("the subfolders", || async {
        folder_at(service, GRAPH, "Inbox/Receipts").await.is_some()
    })
    .await;
    request(
        service,
        Request::SaveRule {
            rule: Box::new(receipts_rule()),
            position: None,
        },
    )
    .await;
    let receipts_remote = env.graph.with_mailbox(None, |m, _| {
        m.folders
            .values()
            .find(|f| f.name == "Receipts")
            .map(|f| f.id.clone())
            .unwrap()
    });
    // Written as an inbox rule (the folder as Graph's id).
    until("the inbox rule on the server", || async {
        env.graph.with_mailbox(None, |m, _| {
            m.rules.iter().any(|r| {
                r["displayName"] == "Receipts"
                    && r["actions"]["moveToFolder"] == receipts_remote.as_str()
            })
        })
    })
    .await;
    until("the placement", || async {
        match service
            .handle(Request::GetExchangeRules {
                account_id: account(),
            })
            .await
        {
            Response::ExchangeRules { status } => status.rules.iter().any(|r| {
                r.placement == nail_api::sieve::RulePlacement::Server && r.server_rule_id.is_some()
            }),
            _ => false,
        }
    })
    .await;
    let Response::ExchangeRules { status } = request(
        service,
        Request::GetExchangeRules {
            account_id: account(),
        },
    )
    .await
    else {
        panic!()
    };
    // The user's own rules are listed next to Nail's.
    assert!(
        status
            .server_rules
            .iter()
            .any(|r| r.name == "Newsletters" && r.nail_rule_id.is_none())
    );
    let placed = status
        .server_rules
        .iter()
        .find(|r| r.name == "Receipts")
        .expect("the placed rule");
    // At the store and UI side the folder is the local one.
    let receipts = folder_at(service, GRAPH, "Inbox/Receipts").await.unwrap();
    assert_eq!(placed.actions.move_to_folder.as_ref(), Some(&receipts.id));
    // Switching a server rule off goes through the Exchange queue.
    let newsletters = status
        .server_rules
        .iter()
        .find(|r| r.name == "Newsletters")
        .unwrap()
        .id
        .clone();
    request(
        service,
        Request::SetExchangeInboxRuleEnabled {
            account_id: account(),
            server_rule_id: newsletters,
            enabled: false,
        },
    )
    .await;
    until("the rule switched off", || async {
        env.graph.with_mailbox(None, |m, _| {
            m.rules
                .iter()
                .any(|r| r["displayName"] == "Newsletters" && r["isEnabled"] == false)
        })
    })
    .await;
    // Deleting the Nail rule removes its inbox rule.
    let Response::Rules { rules } = request(service, Request::ListRules).await else {
        panic!()
    };
    request(
        service,
        Request::DeleteRule {
            rule_id: rules[0].id.clone(),
        },
    )
    .await;
    until("the inbox rule removed", || async {
        env.graph.with_mailbox(None, |m, _| {
            !m.rules.iter().any(|r| r["displayName"] == "Receipts")
        })
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn invitations_are_answered_through_the_meeting_api() {
    use nail_api::scheduling::{InvitationResponse, InvitationTarget};
    let env = setup(false, "").await;
    let service = &env.service;
    let review = || async {
        match service
            .handle(Request::ListEventInstances {
                calendar_ids: None,
                from: "2026-10-01T00:00:00Z".into(),
                to: "2026-10-31T00:00:00Z".into(),
                tz: "UTC".into(),
            })
            .await
        {
            Response::EventInstances { instances, .. } => instances
                .into_iter()
                .find(|i| i.title == "Architecture review"),
            _ => None,
        }
    };
    until("the invitation in the calendar", || async {
        review().await.is_some()
    })
    .await;
    let event = review().await.unwrap();
    request(
        service,
        Request::RespondToInvitation {
            target: InvitationTarget::Event {
                event_id: event.event_id.clone(),
                recurrence_id: None,
            },
            response: InvitationResponse::Accept,
            comment: None,
            send: true,
            calendar_id: None,
        },
    )
    .await;
    until("the answer on Graph", || async {
        env.graph
            .meeting_actions()
            .iter()
            .any(|a| a.action == "accept")
    })
    .await;
    // No iMIP mail: the server answered.
    assert!(env.graph.sent().is_empty());
}
