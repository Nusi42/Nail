//! `Service::handle` for every request, against a store seeded with the demo
//! corpus.

#![cfg(test)]

#[expect(dead_code, reason = "each test crate uses part of the shared helpers")]
mod common;
use common::next_event_except;
use nail_core::testing::{TempService, plain_options};

use nail_api::{
    AccountId, ErrorCode, Event, FolderId, FolderRole, MessageId, PROTOCOL_VERSION, Request,
    Response,
};
use nail_core::demo::{DEMO_ACCOUNT_ID, seed_demo};
use nail_core::{MAX_PAGE_SIZE, Service, ServiceOptions};
use nail_testkit::demo_corpus;
use pretty_assertions::assert_eq;
use tempfile::TempDir;
use tokio::sync::broadcast;

/// A theme with a syntax error (unclosed block) in line 2.
const BROKEN_THEME: &str = "theme \"Broken\" extends=\"default\" {\n    tokens {\n";

struct Fixture {
    service: Service,
    config_dir: TempDir,
    _data_dir: TempDir,
}

fn open(watch_config: bool, locale: Option<&str>) -> Fixture {
    let data_dir = tempfile::tempdir().unwrap();
    let config_dir = tempfile::tempdir().unwrap();
    // Seeded like `nail serve --seed demo`: secrets in memory, no lookups.
    let mut options =
        nail_core::demo::demo_options(ServiceOptions::new(data_dir.path(), config_dir.path()));
    options.watch_config = watch_config;
    options.locale = locale.map(str::to_owned);
    let service = Service::open(options).unwrap();
    seed_demo(service.store(), service.blobs()).unwrap();
    Fixture {
        service,
        config_dir,
        _data_dir: data_dir,
    }
}

fn demo() -> AccountId {
    AccountId::new(DEMO_ACCOUNT_ID)
}

async fn folders(service: &Service) -> Vec<nail_api::Folder> {
    match service
        .handle(Request::ListFolders { account_id: demo() })
        .await
    {
        Response::Folders { folders } => folders,
        other => panic!("unexpected {other:?}"),
    }
}

async fn inbox(service: &Service) -> FolderId {
    folders(service)
        .await
        .into_iter()
        .find(|f| f.role == FolderRole::Inbox)
        .unwrap()
        .id
}

async fn page(service: &Service, folder_id: FolderId, offset: u32, limit: u32) -> Response {
    service
        .handle(Request::ListMessages {
            folder_id,
            offset,
            limit,
        })
        .await
}

fn error_code(response: Response) -> ErrorCode {
    match response {
        Response::Error { code, .. } => code,
        other => panic!("expected an error, got {other:?}"),
    }
}

/// The next event, skipping the editors' history updates (M5d: sent after
/// every change of the config files).
async fn next_event(rx: &mut broadcast::Receiver<Event>) -> Event {
    next_event_except(rx, |e| matches!(e, Event::EditHistoryChanged { .. })).await
}

#[tokio::test]
async fn ping_reports_versions() {
    let f = open(false, None);
    assert_eq!(
        f.service.handle(Request::Ping).await,
        Response::Pong {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            protocol: PROTOCOL_VERSION,
        }
    );
}

#[tokio::test]
async fn get_config_returns_defaults_and_locale() {
    let f = open(false, Some("de"));
    match f.service.handle(Request::GetConfig).await {
        Response::Config {
            theme,
            layout,
            locale,
            platform,
        } => {
            assert_eq!(*theme, nail_config::default_selected_theme());
            assert_eq!(*layout, nail_config::default_layout());
            assert_eq!(locale.as_deref(), Some("de"));
            // No desktop host here: the core's own features only.
            assert_eq!(platform, nail_api::platform::PlatformInfo::current(false));
        }
        other => panic!("unexpected {other:?}"),
    }
    // Every file valid: no M1a error, and an empty list per file (M1b).
    let events = f.service.config_error_events();
    assert_eq!(events.len(), 4, "{events:?}");
    for event in events {
        assert!(
            matches!(&event, Event::ConfigErrors { errors, .. } if errors.is_empty()),
            "{event:?}"
        );
    }
}

#[tokio::test]
async fn lists_accounts_and_folders_with_counts() {
    let f = open(false, None);
    match f.service.handle(Request::ListAccounts).await {
        Response::Accounts { accounts } => {
            // The built-in "All accounts" first (ADR 0012), then the demo.
            assert_eq!(accounts.len(), 2);
            assert_eq!(accounts[0].id, AccountId::all());
            assert_eq!(accounts[0].name, "");
            assert_eq!(accounts[0].kind, nail_api::AccountKind::Virtual);
            assert_eq!(accounts[1].id, demo());
        }
        other => panic!("unexpected {other:?}"),
    }

    let folders = folders(&f.service).await;
    let corpus = demo_corpus();
    let summary: Vec<_> = folders
        .iter()
        .map(|f| (f.path.clone(), f.role, f.total as usize))
        .collect();
    let mut expected: Vec<_> = corpus
        .iter()
        .map(|c| (c.name.clone(), c.role, c.messages.len()))
        .collect();
    expected.sort_by(|a, b| a.0.cmp(&b.0));
    let mut summary_sorted = summary.clone();
    summary_sorted.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(summary_sorted, expected);

    let inbox = folders
        .iter()
        .find(|f| f.role == FolderRole::Inbox)
        .unwrap();
    let corpus_inbox = corpus.iter().find(|c| c.name == "INBOX").unwrap();
    let unread = corpus_inbox
        .messages
        .iter()
        .filter(|m| !m.api_flags().seen)
        .count();
    assert_eq!(inbox.unread as usize, unread);
    assert!(unread > 0);

    assert_eq!(
        error_code(
            f.service
                .handle(Request::ListFolders {
                    account_id: AccountId::new("nope")
                })
                .await
        ),
        ErrorCode::NotFound
    );
}

#[tokio::test]
async fn lists_messages_in_pages_newest_first() {
    let f = open(false, None);
    let inbox = inbox(&f.service).await;
    let Response::Messages {
        folder_id,
        offset,
        total,
        messages: first,
    } = page(&f.service, inbox.clone(), 0, 5).await
    else {
        panic!("expected messages");
    };
    assert_eq!((folder_id, offset, first.len()), (inbox.clone(), 0, 5));
    assert!(total > 5);

    let Response::Messages {
        messages: rest,
        offset,
        ..
    } = page(&f.service, inbox.clone(), 5, 1000).await
    else {
        panic!("expected messages");
    };
    assert_eq!(offset, 5);
    assert_eq!(first.len() + rest.len(), total as usize);
    let ids: std::collections::HashSet<_> = first.iter().chain(&rest).map(|m| &m.id).collect();
    assert_eq!(ids.len(), total as usize);
    // Newest first; a message without a valid date sorts last.
    assert!(first[0].subject.starts_with("Reminder: Nightingale review"));
    assert!(rest.last().unwrap().subject.starts_with("Broken message"));

    let Response::Messages { messages, .. } = page(&f.service, inbox.clone(), 0, 0).await else {
        panic!("expected messages");
    };
    assert!(messages.is_empty(), "{messages:?}");
    let Response::Messages { messages, .. } =
        page(&f.service, inbox.clone(), 0, MAX_PAGE_SIZE + 1).await
    else {
        panic!("expected messages");
    };
    assert_eq!(messages.len(), total as usize);

    assert_eq!(
        error_code(page(&f.service, FolderId::new("nope"), 0, 10).await),
        ErrorCode::NotFound
    );
}

#[tokio::test]
async fn get_message_parses_the_stored_blob() {
    let f = open(false, None);
    let inbox = inbox(&f.service).await;
    let Response::Messages { messages, .. } = page(&f.service, inbox, 0, 100).await else {
        panic!("expected messages");
    };
    let newsletter = messages
        .iter()
        .find(|m| m.subject.contains("garden digest"))
        .expect("newsletter in the inbox");

    let Response::Message { message } = f
        .service
        .handle(Request::GetMessage {
            id: newsletter.id.clone(),
        })
        .await
    else {
        panic!("expected a message");
    };
    assert_eq!(message.id, newsletter.id);
    assert_eq!(message.headers.subject, newsletter.subject);
    assert!(message.remote_content_blocked);
    let html = message.sanitized_html.expect("html body");
    assert!(html.contains("Autumn is here"));
    assert!(!html.contains("track.garden-weekly.example"));
    assert!(!html.contains("<script"));

    assert_eq!(
        error_code(
            f.service
                .handle(Request::GetMessage {
                    id: MessageId::new("nope")
                })
                .await
        ),
        ErrorCode::NotFound
    );
}

#[tokio::test]
async fn blobs_are_served_by_hash() {
    let f = open(false, None);
    let inbox = inbox(&f.service).await;
    let (first_uid, stored) = {
        let uids = f.service.store().folder_uids(&inbox).unwrap();
        let (uid, id) = uids[0].clone();
        (uid, f.service.store().get_message(&id).unwrap())
    };
    assert_eq!(first_uid, 1);
    let hash = stored.blob_hash.unwrap();
    let raw = f.service.blob(hash.as_str()).unwrap();
    assert!(raw.starts_with(b"From:") || raw.windows(5).any(|w| w == b"From:"));

    let err = nail_api::ApiError::from(f.service.blob("not-a-hash").unwrap_err());
    assert_eq!(err.code, ErrorCode::InvalidRequest);
    let missing = "0".repeat(64);
    let err = nail_api::ApiError::from(f.service.blob(&missing).unwrap_err());
    assert_eq!(err.code, ErrorCode::NotFound);
}

#[tokio::test]
async fn sync_now_on_a_local_account_finishes_at_once() {
    let f = open(false, None);
    let mut events = f.service.subscribe();
    assert_eq!(
        f.service
            .handle(Request::SyncNow {
                account_id: Some(demo())
            })
            .await,
        Response::SyncStarted
    );
    assert_eq!(
        next_event(&mut events).await,
        Event::SyncProgress {
            account_id: demo(),
            done: 1,
            total: 1
        }
    );

    // All accounts: nothing is syncable, nothing happens.
    assert_eq!(
        f.service
            .handle(Request::SyncNow { account_id: None })
            .await,
        Response::SyncStarted
    );
    assert_eq!(
        error_code(
            f.service
                .handle(Request::SyncNow {
                    account_id: Some(AccountId::new("nope"))
                })
                .await
        ),
        ErrorCode::NotFound
    );
}

#[tokio::test]
async fn invalid_config_at_start_falls_back_and_is_reported() {
    let _data_dir = tempfile::tempdir().unwrap();
    let _config_dir = tempfile::tempdir().unwrap();
    let temp = TempService::open_with(&[("theme.kdl", BROKEN_THEME)], plain_options);
    let service = temp.service.clone();

    let errors = service.config_error_events();
    let first: Vec<_> = errors
        .iter()
        .filter(|e| matches!(e, Event::ConfigError { .. }))
        .collect();
    assert_eq!(first.len(), 1);
    assert!(matches!(first[0], Event::ConfigError { file, .. } if file == "theme.kdl"));
    assert!(errors.iter().any(|e| matches!(
        e,
        Event::ConfigErrors { file, errors } if file == "theme.kdl" && !errors.is_empty()
    )));
    let Response::Config { theme, .. } = service.handle(Request::GetConfig).await else {
        panic!("expected config");
    };
    assert_eq!(*theme, nail_config::default_selected_theme());
}

#[tokio::test]
async fn config_changes_are_broadcast() {
    let f = open(true, None);
    let mut events = f.service.subscribe();
    let path = f.config_dir.path().join("theme.kdl");

    std::fs::write(
        &path,
        "theme \"Test\" extends=\"default\" {\n    tokens {\n        color \"accent\" \"#ff0066\"\n    }\n}\n",
    )
    .unwrap();
    // `color "accent"` is an appearance override: ThemeChanged comes first.
    let mut event = next_event(&mut events).await;
    if matches!(event, Event::ThemeChanged { .. }) {
        event = next_event(&mut events).await;
    }
    match event {
        Event::ConfigChanged { theme, .. } => {
            let dump = format!("{theme:?}");
            assert!(dump.contains("#ff0066"), "{dump}");
        }
        other => panic!("unexpected {other:?}"),
    }

    std::fs::write(&path, BROKEN_THEME).unwrap();
    match next_event(&mut events).await {
        Event::ConfigError { file, line, .. } => {
            assert_eq!(file, "theme.kdl");
            assert!(line >= 1);
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn m1a_requests_answer_without_accounts() {
    let fx = open(false, None);
    assert_eq!(
        fx.service.handle(Request::GetAccountsConfig).await,
        Response::AccountsConfig {
            config: nail_api::AccountsConfig::default()
        }
    );
    assert_eq!(
        fx.service.handle(Request::ListOutbox).await,
        Response::Outbox { entries: vec![] }
    );
    assert_eq!(
        fx.service.handle(Request::ListAccountStatuses).await,
        Response::AccountStatuses { statuses: vec![] }
    );
    // "All accounts" shows the demo account's folders merged by role.
    let Response::Folders { folders } = fx
        .service
        .handle(Request::ListFolders {
            account_id: AccountId::all(),
        })
        .await
    else {
        panic!("expected folders");
    };
    let inbox = folders
        .iter()
        .find(|f| f.role == FolderRole::Inbox)
        .expect("a merged inbox");
    assert_eq!(inbox.id.as_str(), "all/inbox");
    assert_eq!(inbox.total, inbox_total(&fx.service).await);
}

async fn inbox_total(service: &Service) -> u32 {
    folders(service)
        .await
        .into_iter()
        .find(|f| f.role == FolderRole::Inbox)
        .unwrap()
        .total
}

/// Fails every operation (to test `RetryOp`).
struct FailAll;

impl nail_store::Replayer for FailAll {
    fn replay(
        &self,
        _op: &nail_store::QueuedOp,
    ) -> impl Future<Output = nail_store::ReplayOutcome> + Send {
        std::future::ready(nail_store::ReplayOutcome::Fail("rejected".into()))
    }
}

#[tokio::test]
async fn retry_op_requeues_a_failed_operation() {
    let fx = open(false, None);
    let Response::Messages { messages, .. } =
        page(&fx.service, inbox(&fx.service).await, 0, 1).await
    else {
        panic!("expected messages");
    };
    let message = messages[0].id.clone();
    let store = fx.service.store().clone();
    let queued = store
        .enqueue(&nail_store::Operation::UpdateFlags {
            message_id: message,
            change: nail_store::FlagChange {
                flagged: Some(true),
                ..nail_store::FlagChange::default()
            },
        })
        .unwrap();
    store
        .replay_account(&demo(), &FailAll, nail_store::ReplayPolicy::default())
        .await
        .unwrap();
    let op_id = nail_api::OpId::new(format!("op-{}", queued.op.id.0));

    let mut events = fx.service.subscribe();
    assert_eq!(
        fx.service
            .handle(Request::RetryOp {
                op_id: op_id.clone()
            })
            .await,
        Response::Queued {
            op_ids: vec![op_id.clone()]
        }
    );
    match next_event(&mut events).await {
        Event::OpStatus { status } => {
            assert_eq!(status.op_id, op_id);
            assert_eq!(status.state, nail_api::OpState::Pending);
        }
        other => panic!("unexpected {other:?}"),
    }
    // Only failed operations can be retried; unknown ids are not found.
    assert_eq!(
        error_code(fx.service.handle(Request::RetryOp { op_id }).await),
        ErrorCode::Conflict
    );
    assert_eq!(
        error_code(
            fx.service
                .handle(Request::RetryOp {
                    op_id: "bogus".into()
                })
                .await
        ),
        ErrorCode::NotFound
    );
}

#[tokio::test]
async fn ui_context_and_saved_searches() {
    let fx = open(false, None);
    assert_eq!(
        fx.service.handle(Request::GetUiContext).await,
        Response::UiContext {
            context: nail_api::UiContext::default()
        }
    );
    let context = nail_api::UiContext {
        message_id: Some("m1".into()),
        selected_message_ids: vec!["m1".into()],
        ..nail_api::UiContext::default()
    };
    assert_eq!(
        fx.service
            .handle(Request::ReportUiContext {
                context: context.clone()
            })
            .await,
        Response::Done
    );
    assert_eq!(fx.service.ui_context(), context);

    let mut events = fx.service.subscribe();
    let search = nail_api::SavedSearch {
        id: None,
        name: "Unread".into(),
        query: "is:unread".into(),
        account_id: None,
        folder_id: None,
        sort: nail_api::search::SearchSort::Date,
        position: 0,
    };
    let Response::SavedSearchSaved { search: saved } = fx
        .service
        .handle(Request::SaveSavedSearch {
            search: search.clone(),
        })
        .await
    else {
        panic!("expected a saved search");
    };
    assert!(saved.id.is_some());
    assert_eq!(next_event(&mut events).await, Event::SavedSearchesChanged);
    assert_eq!(
        fx.service.handle(Request::ListSavedSearches).await,
        Response::SavedSearches {
            searches: vec![saved.clone()]
        }
    );
    assert_eq!(
        error_code(
            fx.service
                .handle(Request::SaveSavedSearch {
                    search: nail_api::SavedSearch {
                        query: "from:".into(),
                        ..search
                    }
                })
                .await
        ),
        ErrorCode::InvalidRequest
    );
    let id = saved.id.unwrap();
    assert_eq!(
        fx.service
            .handle(Request::DeleteSavedSearch {
                search_id: id.clone()
            })
            .await,
        Response::Done
    );
    assert_eq!(
        error_code(
            fx.service
                .handle(Request::DeleteSavedSearch { search_id: id })
                .await
        ),
        ErrorCode::NotFound
    );
}

#[tokio::test]
async fn m1b_package_requests_are_answered_by_the_core() {
    let fx = open(false, None);
    for (request, expected) in [
        (
            Request::RenderPreview {
                markdown: "# x".into(),
                identity_id: None,
            },
            "PreviewRendered",
        ),
        (Request::ListRules, "Rules"),
        (Request::ListMcpClients, "McpClients"),
        (
            Request::ListConfigBackups {
                file: nail_api::ConfigFile::Theme,
            },
            "ConfigBackups",
        ),
    ] {
        let response = serde_json::to_value(fx.service.handle(request).await).unwrap();
        assert_eq!(response["type"], expected, "{response}");
    }
}

/// Regression (M1b): the demo seed goes straight into the store, so `nail
/// serve --seed demo` found nothing until the index was rebuilt.
#[tokio::test]
async fn a_seeded_store_is_searchable_after_a_rebuild() {
    let fx = open(false, None);
    let search = |text: &str| Request::Search {
        query: nail_api::SearchQuery {
            text: text.into(),
            account_id: None,
            folder_id: None,
            offset: 0,
            limit: 10,
            sort: nail_api::search::SearchSort::Date,
        },
    };
    let total = |response: Response| match response {
        Response::SearchResults { total, .. } => total,
        other => panic!("unexpected {other:?}"),
    };
    assert_eq!(total(fx.service.handle(search("welcome")).await), 0);
    assert!(fx.service.rebuild_search_index().unwrap() > 0);
    assert!(total(fx.service.handle(search("welcome")).await) >= 1);
}

#[tokio::test]
async fn old_mcp_audit_entries_and_rule_runs_are_purged() {
    use nail_api::mcp::{McpCallKind, McpOutcome};
    // Without engines: the purge at start must not race the test.
    let data_dir = tempfile::tempdir().unwrap();
    let config_dir = tempfile::tempdir().unwrap();
    let mut options = ServiceOptions::new(data_dir.path(), config_dir.path());
    options.watch_config = false;
    options.start_engines = false;
    let service = Service::open(options).unwrap();
    let day = 86_400;
    // Real time: the service purges at start as well.
    let now = nail_util::time::unix_now();
    let store = service.store();
    for at in [now - 100 * day, now - day] {
        store
            .append_mcp_audit(&nail_store::McpAuditRecord {
                client_id: None,
                client_name: "agent".into(),
                at,
                kind: McpCallKind::Connect,
                name: "/mcp".into(),
                outcome: McpOutcome::Denied,
                detail: None,
            })
            .unwrap();
        let run = store
            .start_rule_run(&"r".into(), "R", nail_api::rules::RuleRunCause::Manual, at)
            .unwrap();
        store
            .finish_rule_run(&run, nail_api::rules::RuleRunState::Done, 0, None, at)
            .unwrap();
    }
    assert_eq!(service.purge_history(now).await.unwrap(), (1, 1));
    assert_eq!(service.purge_history(now).await.unwrap(), (0, 0));
    assert_eq!(store.list_rule_runs(None, 10).unwrap().len(), 1);
}

/// M2a (ADR 0029): the security settings and vault status are served by
/// the core; every other security request answers `unsupported` until the
/// integration wires the crypto crates in.
#[tokio::test]
async fn m2a_security_requests() {
    use nail_api::security::{KeyLookupPolicy, KeyLookupSource, SecuritySettings, VaultStatus};

    let fx = open(false, None);
    assert_eq!(
        fx.service.handle(Request::GetSecuritySettings).await,
        Response::SecuritySettings {
            settings: SecuritySettings::default()
        }
    );
    let mut events = fx.service.subscribe();
    let wanted = SecuritySettings {
        key_lookup: KeyLookupPolicy::Off,
        lookup_sources: vec![
            KeyLookupSource::KeysOpenpgpOrg,
            KeyLookupSource::Wkd,
            KeyLookupSource::KeysOpenpgpOrg,
        ],
        ..SecuritySettings::default()
    };
    let Response::SecuritySettings { settings } = fx
        .service
        .handle(Request::UpdateSecuritySettings { settings: wanted })
        .await
    else {
        panic!("expected settings");
    };
    // Duplicates are dropped, the order is kept.
    assert_eq!(
        settings.lookup_sources,
        [KeyLookupSource::KeysOpenpgpOrg, KeyLookupSource::Wkd]
    );
    assert_eq!(
        next_event(&mut events).await,
        Event::SecuritySettingsChanged
    );
    assert_eq!(
        fx.service.handle(Request::GetSecuritySettings).await,
        Response::SecuritySettings {
            settings: settings.clone()
        }
    );
    // Decrypted indexing only for configured accounts.
    assert_eq!(
        error_code(
            fx.service
                .handle(Request::UpdateSecuritySettings {
                    settings: SecuritySettings {
                        index_decrypted: vec!["nope".into()],
                        ..settings
                    }
                })
                .await
        ),
        ErrorCode::InvalidRequest
    );

    assert_eq!(
        fx.service.handle(Request::GetVaultStatus).await,
        Response::VaultStatus {
            status: VaultStatus::plain()
        }
    );
    // M2a is wired (ADR 0034): an empty keyring, an unknown identity, and
    // online lookups refused while the setting is off.
    assert_eq!(
        fx.service
            .handle(Request::ListKeys {
                filter: nail_api::security::KeyFilter::default(),
            })
            .await,
        Response::Keys { keys: vec![] }
    );
    assert_eq!(
        error_code(
            fx.service
                .handle(Request::CheckRecipientKeys {
                    identity_id: "ada@example.org".into(),
                    recipients: vec!["bob@example.org".into()],
                    lookup: false,
                })
                .await
        ),
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        error_code(
            fx.service
                .handle(Request::LookupKeys {
                    email: "bob@example.org".into(),
                    sources: vec![],
                })
                .await
        ),
        ErrorCode::Unsupported
    );
}

/// M2b (ADR 0036): the remote-UI settings (validated; internet mode
/// refused), users, devices, sessions, pairing codes, the audit log and the
/// remote-content allow-lists are served from the store; hashing, TOTP and
/// pairing codes by nail-remote-auth (WP45); certificates need a host.
#[tokio::test]
async fn m2b_remote_requests() {
    use nail_api::remote::{
        RemoteAccessPreset, RemoteSessionFilter, RemoteTlsMode, RemoteUiSettings, RemoteUiState,
        RemoteUserUpdate,
    };
    use nail_api::remote_content::RemoteContentScope;

    let fx = open(false, None);
    let Response::RemoteUi { settings, status } =
        fx.service.handle(Request::GetRemoteUiSettings).await
    else {
        panic!("expected the remote-UI settings");
    };
    assert_eq!(settings, RemoteUiSettings::default());
    assert_eq!(status.state, RemoteUiState::Stopped);
    assert!(status.internet_mode_locked);

    let mut events = fx.service.subscribe();
    let lan = RemoteUiSettings {
        enabled: true,
        bind_address: "0.0.0.0".into(),
        ..RemoteUiSettings::default()
    };
    let Response::RemoteUi { settings, .. } = fx
        .service
        .handle(Request::UpdateRemoteUiSettings {
            settings: lan.clone(),
        })
        .await
    else {
        panic!("expected the saved settings");
    };
    assert_eq!(settings, lan);
    assert!(matches!(
        next_event(&mut events).await,
        Event::RemoteUiStatusChanged { .. }
    ));
    // TLS is required off loopback; internet mode stays locked.
    let plain_lan = RemoteUiSettings {
        tls: RemoteTlsMode::Off,
        ..lan.clone()
    };
    assert_eq!(
        error_code(
            fx.service
                .handle(Request::UpdateRemoteUiSettings {
                    settings: plain_lan
                })
                .await
        ),
        ErrorCode::InvalidRequest
    );
    let internet = RemoteUiSettings {
        internet_mode: true,
        ..lan.clone()
    };
    assert_eq!(
        error_code(
            fx.service
                .handle(Request::UpdateRemoteUiSettings { settings: internet })
                .await
        ),
        ErrorCode::Forbidden
    );
    let Response::RemoteUi { settings, .. } = fx.service.handle(Request::GetRemoteUiSettings).await
    else {
        panic!("expected the remote-UI settings");
    };
    assert_eq!(settings, lan, "refused settings are not stored");

    // Users.
    let Response::RemoteUserSaved { user } = fx
        .service
        .handle(Request::CreateRemoteUser {
            name: "ada".into(),
            display_name: None,
            grant: RemoteAccessPreset::NoSend.grant(),
        })
        .await
    else {
        panic!("expected the new user");
    };
    assert_eq!(next_event(&mut events).await, Event::RemoteAccessChanged);
    assert!(!user.has_password && user.passkeys.is_empty());
    assert_eq!(
        error_code(
            fx.service
                .handle(Request::CreateRemoteUser {
                    name: "ADA".into(),
                    display_name: None,
                    grant: nail_api::remote::RemoteGrant::default(),
                })
                .await
        ),
        ErrorCode::Conflict
    );
    let Response::RemoteUserSaved { user } = fx
        .service
        .handle(Request::UpdateRemoteUser {
            user_id: user.id.clone(),
            update: RemoteUserUpdate {
                display_name: Some("Ada L.".into()),
                ..RemoteUserUpdate::default()
            },
        })
        .await
    else {
        panic!("expected the changed user");
    };
    assert_eq!(user.display_name.as_deref(), Some("Ada L."));
    let Response::RemoteUsers { users } = fx.service.handle(Request::ListRemoteUsers).await else {
        panic!("expected users");
    };
    assert_eq!(users.len(), 1);

    // A session paired by the (future) auth crate, revoked by the desktop.
    let store = fx.service.store();
    let device = store
        .create_remote_device(
            &nail_store::NewRemoteDevice {
                user_id: user.id.clone(),
                name: "Phone".into(),
                secret_hash: nail_store::hash_remote_secret("device"),
                grant: RemoteAccessPreset::Full.grant(),
                user_agent: None,
                ip: None,
            },
            1,
        )
        .unwrap();
    let now = nail_util::time::unix_now();
    let session = store
        .create_remote_session(
            &nail_store::NewRemoteSession {
                token_hash: nail_store::hash_remote_secret("token"),
                user_id: user.id.clone(),
                device_id: device.id.clone(),
                auth_method: nail_api::remote::RemoteAuthMethod::Passkey,
                grant: RemoteAccessPreset::Full.grant(),
                expires_at: now + 3_600,
                ip: None,
                user_agent: None,
            },
            now,
        )
        .unwrap();
    let Response::RemoteSessions { sessions } = fx
        .service
        .handle(Request::ListRemoteSessions {
            filter: RemoteSessionFilter::default(),
        })
        .await
    else {
        panic!("expected sessions");
    };
    assert_eq!(sessions.len(), 1);
    assert!(
        !sessions[0].grant.mail.has(nail_api::McpScope::Send),
        "the session is narrowed to the user's no-send grant"
    );
    let Response::RemoteUi { status, .. } = fx.service.handle(Request::GetRemoteUiSettings).await
    else {
        panic!("expected the remote-UI settings");
    };
    assert_eq!(status.active_sessions, 1);
    let mut events = fx.service.subscribe();
    assert_eq!(
        fx.service
            .handle(Request::RevokeRemoteSession {
                session_id: session.id.clone()
            })
            .await,
        Response::Done
    );
    assert_eq!(
        next_event(&mut events).await,
        Event::RemoteSessionEnded {
            session_id: session.id.clone(),
            reason: nail_api::remote::SessionEndReason::Revoked,
        }
    );
    assert_eq!(
        fx.service
            .handle(Request::RevokeRemoteDevice {
                device_id: device.id.clone()
            })
            .await,
        Response::Done
    );
    let Response::RemoteDevices { devices } = fx
        .service
        .handle(Request::ListRemoteDevices { user_id: None })
        .await
    else {
        panic!("expected devices");
    };
    assert!(devices[0].revoked_at.is_some());
    assert_eq!(
        fx.service.handle(Request::ListPairingCodes).await,
        Response::PairingCodes { codes: vec![] }
    );
    assert_eq!(
        fx.service
            .handle(Request::ListRemoteAudit {
                user_id: None,
                before: None,
                limit: 10,
            })
            .await,
        Response::RemoteAudit { entries: vec![] }
    );

    // Credentials and pairing codes are served by nail-remote-auth (WP45;
    // details in tests/remote.rs); certificates need the remote-UI host
    // (nail-server), which this bare service does not have.
    assert!(matches!(
        fx.service
            .handle(Request::SetRemotePassword {
                user_id: user.id.clone(),
                password: Some("correct horse battery staple".into()),
            })
            .await,
        Response::RemoteUserSaved { .. }
    ));
    assert!(matches!(
        fx.service
            .handle(Request::CreatePairingCode {
                user_id: user.id.clone(),
                device_name: None,
                grant: None,
                ttl_minutes: None,
            })
            .await,
        Response::PairingCodeCreated { .. }
    ));
    assert_eq!(
        error_code(
            fx.service
                .handle(Request::RegenerateRemoteCertificate)
                .await
        ),
        ErrorCode::Unsupported
    );
    assert_eq!(
        fx.service
            .handle(Request::DeleteRemoteUser {
                user_id: user.id.clone()
            })
            .await,
        Response::Done
    );
    assert_eq!(
        error_code(
            fx.service
                .handle(Request::DeleteRemoteUser { user_id: user.id })
                .await
        ),
        ErrorCode::NotFound
    );

    // Remote content: per message, sender and domain.
    let Response::Messages { messages, .. } =
        page(&fx.service, inbox(&fx.service).await, 0, 1).await
    else {
        panic!("expected messages");
    };
    let message = &messages[0];
    let sender = message.from.as_ref().unwrap().email.to_lowercase();
    let mut events = fx.service.subscribe();
    let Response::RemoteContentAllowed { entry } = fx
        .service
        .handle(Request::AllowRemoteContent {
            message_id: message.id.clone(),
            scope: RemoteContentScope::Sender,
        })
        .await
    else {
        panic!("expected the allow-list entry");
    };
    assert_eq!(entry.value, sender);
    assert_eq!(
        next_event(&mut events).await,
        Event::RemoteContentPolicyChanged
    );
    let Response::RemoteContentAllowed { entry } = fx
        .service
        .handle(Request::AllowRemoteContent {
            message_id: message.id.clone(),
            scope: RemoteContentScope::Message,
        })
        .await
    else {
        panic!("expected the allow-list entry");
    };
    assert_eq!(entry.label.as_deref(), Some(message.subject.as_str()));
    let Response::RemoteContentAllows { entries } =
        fx.service.handle(Request::ListRemoteContentAllows).await
    else {
        panic!("expected the allow-lists");
    };
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].scope, RemoteContentScope::Message);
    assert_eq!(
        fx.service
            .handle(Request::RemoveRemoteContentAllow {
                scope: RemoteContentScope::Sender,
                value: sender,
            })
            .await,
        Response::Done
    );
    assert_eq!(
        error_code(
            fx.service
                .handle(Request::AllowRemoteContent {
                    message_id: "nope".into(),
                    scope: RemoteContentScope::Domain,
                })
                .await
        ),
        ErrorCode::NotFound
    );
}
