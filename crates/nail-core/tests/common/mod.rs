//! Helpers shared by the core's integration tests (wave Q2, ADR 0171):
//! polling, the answers the tests read, and the demo-multi fixture. Quiet
//! services on temporary directories come from `nail_core::testing`.

use std::future::Future;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use nail_api::accounts::{SecretKind, SecretRef};
use nail_api::engine::{MemorySecretStore, SecretStore};
use nail_api::plugins::{PluginGrant, PluginPackageSource, PluginPermission};
use nail_api::remote::{AuthRequest, AuthResponse, DEVICE_COOKIE, RemoteGrant, SESSION_COOKIE};
use nail_api::rules::{Condition, FolderRef, Rule, RuleAction, RuleTrigger, TextMatch, TextOp};

use nail_api::{
    AccountId, AccountState, DraftId, Event, Folder, FolderId, FolderRole, MessageBody, MessageId,
    MessageSummary, OpId, Request, Response,
};
use nail_core::demo::{DEMO_ACCOUNT_ID, DemoMulti, start_demo_multi};
use nail_core::testing::TempService;
use nail_core::{EngineTiming, Service, ServiceOptions};
use nail_remote_auth::{ClientMeta, Cookies, RemoteSessionCtx};
use nail_testkit_net::{MockOAuthConfig, MockOAuthServer};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::sync::broadcast;

/// The subject of the demo conversation the conversation tests act on.
pub(crate) const NIGHTINGALE: &str = "Project Nightingale kickoff";

/// How long a test waits for the core (sync passes, replays, events).
pub(crate) const TIMEOUT: Duration = Duration::from_secs(30);

/// How often a waiting test polls the core.
const POLL: Duration = Duration::from_millis(50);

/// Waits until `check` holds.
pub(crate) async fn eventually<F, Fut>(what: &str, mut check: F)
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
        tokio::time::sleep(POLL).await;
    }
}

/// Waits until `check` yields a value.
pub(crate) async fn until_some<T, F, Fut>(what: &str, mut check: F) -> T
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Option<T>>,
{
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    loop {
        if let Some(value) = check().await {
            return value;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for: {what}"
        );
        tokio::time::sleep(POLL).await;
    }
}

/// The next event (at most 10 s away).
pub(crate) async fn next_event(rx: &mut broadcast::Receiver<Event>) -> Event {
    tokio::time::timeout(Duration::from_secs(10), rx.recv())
        .await
        .expect("no event within 10 s")
        .unwrap()
}

/// The core's answer.
pub(crate) async fn request(service: &Service, request: Request) -> Response {
    service.handle(request).await
}

/// The core's answer; an error answer fails the test.
pub(crate) async fn ok(service: &Service, request: Request) -> Response {
    let response = service.handle(request).await;
    if let Response::Error { message, .. } = &response {
        panic!("request failed: {message}");
    }
    response
}

/// The folders of `account`.
pub(crate) async fn folders(service: &Service, account: &str) -> Vec<Folder> {
    match service
        .handle(Request::ListFolders {
            account_id: AccountId::new(account),
        })
        .await
    {
        Response::Folders { folders } => folders,
        other => panic!("unexpected {other:?}"),
    }
}

/// The folder of `account` with `role`, if it has one.
pub(crate) async fn folder(service: &Service, account: &str, role: FolderRole) -> Option<Folder> {
    folders(service, account)
        .await
        .into_iter()
        .find(|f| f.role == role)
}

/// The folder of `account` at `path`, if there is one.
pub(crate) async fn folder_at(service: &Service, account: &str, path: &str) -> Option<Folder> {
    folders(service, account)
        .await
        .into_iter()
        .find(|f| f.path == path)
}

/// The messages of a folder (the first 500).
pub(crate) async fn messages(service: &Service, folder: &FolderId) -> Vec<MessageSummary> {
    match service
        .handle(Request::ListMessages {
            folder_id: folder.clone(),
            offset: 0,
            limit: 500,
        })
        .await
    {
        Response::Messages { messages, .. } => messages,
        other => panic!("unexpected {other:?}"),
    }
}

/// The messages in the folder of `account` with `role` (none while the
/// folder is not there yet).
pub(crate) async fn messages_in(
    service: &Service,
    account: &str,
    role: FolderRole,
) -> Vec<MessageSummary> {
    match folder(service, account, role).await {
        Some(folder) => messages(service, &folder.id).await,
        None => Vec::new(),
    }
}

/// The subjects in the folder of `account` with `role`.
pub(crate) async fn subjects(service: &Service, account: &str, role: FolderRole) -> Vec<String> {
    messages_in(service, account, role)
        .await
        .into_iter()
        .map(|m| m.subject)
        .collect()
}

/// How many messages the inbox of `account` holds.
pub(crate) async fn inbox_count(service: &Service, account: &str) -> usize {
    messages_in(service, account, FolderRole::Inbox).await.len()
}

/// The message in the folder of `account` with `role` whose subject
/// contains `subject`.
pub(crate) async fn find(
    service: &Service,
    account: &str,
    role: FolderRole,
    subject: &str,
) -> MessageSummary {
    messages_in(service, account, role)
        .await
        .into_iter()
        .find(|m| m.subject.contains(subject))
        .unwrap_or_else(|| panic!("no message {subject:?} in {account} {role:?}"))
}

/// The message with exactly this subject.
pub(crate) fn by_subject<'a>(list: &'a [MessageSummary], subject: &str) -> &'a MessageSummary {
    list.iter()
        .find(|m| m.subject == subject)
        .unwrap_or_else(|| panic!("no message {subject:?}"))
}

/// A message's body as the reader gets it.
pub(crate) async fn body(service: &Service, id: &MessageId) -> MessageBody {
    match service.handle(Request::GetMessage { id: id.clone() }).await {
        Response::Message { message } => *message,
        other => panic!("unexpected {other:?}"),
    }
}

/// Every account's connection state.
pub(crate) async fn statuses(service: &Service) -> Vec<(AccountId, AccountState)> {
    match service.handle(Request::ListAccountStatuses).await {
        Response::AccountStatuses { statuses } => statuses
            .into_iter()
            .map(|s| (s.account_id, s.state))
            .collect(),
        other => panic!("unexpected {other:?}"),
    }
}

/// Whether `account` is online (finished a sync pass).
pub(crate) async fn online(service: &Service, account: &str) -> bool {
    statuses(service)
        .await
        .iter()
        .any(|(id, state)| id.as_str() == account && *state == AccountState::Online)
}

/// The operations a write queued.
pub(crate) fn op_ids(response: Response) -> Vec<OpId> {
    match response {
        Response::Queued { op_ids } => op_ids,
        other => panic!("expected Queued, got {other:?}"),
    }
}

/// Saves a draft; returns its id.
pub(crate) async fn save(service: &Service, draft: nail_api::Draft) -> DraftId {
    match service.handle(Request::SaveDraft { draft }).await {
        Response::DraftSaved { draft_id } => draft_id,
        other => panic!("unexpected {other:?}"),
    }
}

/// A service on the demo-multi mock servers (IMAP, POP3, SMTP, DAV,
/// Gmail, Exchange; `accounts.kdl` written by the demo).
pub(crate) struct Multi {
    pub(crate) service: Service,
    pub(crate) demo: DemoMulti,
    pub(crate) config_dir: TempDir,
    pub(crate) data_dir: TempDir,
}

impl Multi {
    /// Starts the mock servers and a service with their accounts.
    pub(crate) async fn start() -> Self {
        let demo = start_demo_multi().await.unwrap();
        Self::with(demo, |_, _| {})
    }

    /// A service on `demo`'s accounts; `prepare` sees the demo and the
    /// config directory before the service opens.
    pub(crate) fn with(demo: DemoMulti, prepare: impl FnOnce(&DemoMulti, &Path)) -> Self {
        let data_dir = tempfile::tempdir().unwrap();
        let config_dir = tempfile::tempdir().unwrap();
        demo.write_config(config_dir.path()).unwrap();
        prepare(&demo, config_dir.path());
        let service = Service::open(Self::options_for(&demo, &data_dir, &config_dir)).unwrap();
        Self {
            service,
            demo,
            config_dir,
            data_dir,
        }
    }

    /// The demo's service options on these directories (a restart opens
    /// a service with them).
    pub(crate) fn options(&self) -> ServiceOptions {
        Self::options_for(&self.demo, &self.data_dir, &self.config_dir)
    }

    fn options_for(demo: &DemoMulti, data: &TempDir, config: &TempDir) -> ServiceOptions {
        demo.service_options(ServiceOptions::new(data.path(), config.path()))
    }

    /// An hour from the demo clock's now (RFC 3339).
    pub(crate) fn in_an_hour(&self) -> String {
        let now = nail_triage::Clock::unix(&*self.demo.clock);
        chrono::DateTime::from_timestamp(now + 3600, 0)
            .unwrap()
            .to_rfc3339()
    }

    /// Runs a demo-multi test control.
    pub(crate) fn control(&self, command: &str, args: &Value) -> Value {
        self.demo.control(command, args).unwrap()
    }

    /// The messages the mock SMTP server received (their data).
    pub(crate) fn smtp_data(&self) -> Vec<String> {
        self.control("smtp-messages", &json!({}))
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["data"].as_str().unwrap().to_owned())
            .collect()
    }

    /// The subjects of the messages the mock SMTP server received.
    pub(crate) fn smtp_subjects(&self) -> Vec<String> {
        self.smtp_data()
            .iter()
            .filter_map(|d| {
                d.lines()
                    .find_map(|l| l.strip_prefix("Subject: ").map(str::to_owned))
            })
            .collect()
    }

    /// The subjects in `mailbox` of the IMAP `account` on its mock server.
    pub(crate) fn server_subjects(&self, account: &str, mailbox: &str) -> Vec<String> {
        self.control(
            "imap-messages",
            &json!({ "account": account, "mailbox": mailbox }),
        )
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["subject"].as_str().unwrap_or_default().to_owned())
        .collect()
    }
}

/// A mock authorization server that has issued the refresh tokens
/// `(token, user, scope)`.
pub(crate) async fn mock_oauth(tokens: &[(&str, &str, &str)]) -> MockOAuthServer {
    let oauth = MockOAuthServer::start(MockOAuthConfig::default())
        .await
        .unwrap();
    for (token, user, scope) in tokens {
        oauth.add_refresh_token(token, user, scope);
    }
    oauth
}

/// The `oauth-provider` node `name` of `oauth` (client `nail-test-client`)
/// asking for `scope`.
pub(crate) fn oauth_provider(name: &str, oauth: &MockOAuthServer, scope: &str) -> String {
    format!(
        "oauth-provider \"{name}\" client-id=\"nail-test-client\" auth-url=\"{auth}\" token-url=\"{token}\" {{\n    scopes \"{scope}\"\n}}\n",
        auth = oauth.authorize_url(),
        token = oauth.token_url(),
    )
}

/// A service for API engine tests: `accounts_kdl`, the account secrets
/// `(account, kind, value)` in memory, engines on with fast timing.
pub(crate) fn api_service(accounts_kdl: &str, secrets: &[(&str, SecretKind, &str)]) -> TempService {
    let store = Arc::new(MemorySecretStore::new());
    for (account, kind, value) in secrets {
        store
            .set(
                &SecretRef::default_for(&AccountId::new(*account), *kind),
                value,
            )
            .unwrap();
    }
    TempService::quiet_with(&[("accounts.kdl", accounts_kdl)], |o| {
        o.start_engines = true;
        o.secrets = Some(store);
        o.timing = EngineTiming::fast();
    })
}

/// The folder of the demo account with `role` (from the store).
pub(crate) fn demo_folder(service: &Service, role: FolderRole) -> Folder {
    service
        .store()
        .list_folders(&AccountId::new(DEMO_ACCOUNT_ID))
        .unwrap()
        .into_iter()
        .find(|folder| folder.role == role)
        .unwrap()
}

/// A remote session of a new remote user `name` with `grant`, paired from
/// a phone (`192.168.1.20`) like the remote UI does it.
pub(crate) async fn remote_session(
    service: &Service,
    name: &str,
    grant: RemoteGrant,
) -> RemoteSessionCtx {
    let user = match service
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
    let code = match service
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
    let auth = service.remote_auth();
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

/// Changes the configuration of `account` (`UpdateAccount`, no secrets).
pub(crate) async fn update_account(
    service: &Service,
    account: &str,
    change: impl FnOnce(&mut nail_api::AccountConfig),
) {
    let Response::AccountsConfig { config } = service.handle(Request::GetAccountsConfig).await
    else {
        panic!("no config")
    };
    let mut config = config.account(&AccountId::new(account)).unwrap().clone();
    change(&mut config);
    assert!(matches!(
        service
            .handle(Request::UpdateAccount {
                config: Box::new(config),
                secrets: vec![],
            })
            .await,
        Response::AccountSaved { .. }
    ));
}

/// Every file under `dir`, recursively.
pub(crate) fn files_under(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(files_under(&path));
        } else {
            out.push(path);
        }
    }
    out
}

/// Saves `draft` and sends it at once (no undo delay).
pub(crate) async fn send_now(service: &Service, draft: nail_api::Draft) {
    let draft_id = save(service, draft).await;
    assert!(matches!(
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
        .await,
        Response::SendQueued { .. }
    ));
}

/// Waits for the next event `pick` accepts (within [`TIMEOUT`]).
pub(crate) async fn next_matching<T>(
    events: &mut broadcast::Receiver<Event>,
    what: &str,
    mut pick: impl FnMut(Event) -> Option<T>,
) -> T {
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    loop {
        let event = tokio::time::timeout_at(deadline, events.recv())
            .await
            .unwrap_or_else(|_| panic!("timed out waiting for: {what}"))
            .unwrap();
        if let Some(value) = pick(event) {
            return value;
        }
    }
}

/// A rule over mail whose subject contains `subject`.
pub(crate) fn subject_rule(
    name: &str,
    triggers: Vec<RuleTrigger>,
    subject: &str,
    actions: Vec<RuleAction>,
) -> Rule {
    Rule {
        id: "".into(),
        name: name.into(),
        enabled: true,
        triggers,
        scope: Vec::new(),
        conditions: Condition::All {
            conditions: vec![Condition::Subject {
                matches: TextMatch {
                    op: TextOp::Contains,
                    value: subject.into(),
                    case_sensitive: false,
                },
            }],
        },
        actions,
        source: None,
    }
}

/// Into the Archive, flagged: what the rule tests' rules do.
pub(crate) fn archive_and_flag() -> Vec<RuleAction> {
    vec![
        RuleAction::Move {
            folder: FolderRef::Role {
                role: FolderRole::Archive,
                account_id: None,
            },
        },
        RuleAction::Flag { flagged: true },
    ]
}

/// Runs a rule by hand over its scope.
pub(crate) async fn run_rule(
    service: &Service,
    rule: &nail_api::RuleId,
) -> nail_api::rules::RuleRun {
    let Response::RuleRan { run } = ok(
        service,
        Request::RunRule {
            rule_id: rule.clone(),
            message_ids: Vec::new(),
            folder_id: None,
        },
    )
    .await
    else {
        panic!("rule run expected")
    };
    *run
}

/// Saves a rule (at the end of the list); returns it as stored.
pub(crate) async fn save_rule(service: &Service, rule: Rule) -> Rule {
    match ok(
        service,
        Request::SaveRule {
            rule: Box::new(rule),
            position: None,
        },
    )
    .await
    {
        Response::RuleSaved { rule } => *rule,
        other => panic!("unexpected {other:?}"),
    }
}

/// Installs and enables the plug-in package at `path` with every one of
/// `permissions` granted; returns the core's answer.
pub(crate) async fn install_package(
    service: &Service,
    path: &Path,
    permissions: &[&str],
) -> Response {
    let source = PluginPackageSource::File {
        path: path.display().to_string(),
    };
    let sha = match service
        .handle(Request::PreviewPluginInstall {
            source: source.clone(),
        })
        .await
    {
        Response::PluginInstallPreview { preview } => preview.package_sha256,
        other => panic!("{other:?}"),
    };
    let grants = permissions
        .iter()
        .map(|p| PluginGrant::all(PluginPermission::parse(p).unwrap()))
        .collect();
    service
        .handle(Request::InstallPlugin {
            source,
            package_sha256: sha,
            grants,
            enable: true,
        })
        .await
}

/// The next event `skip` does not skip (each within 10 s).
pub(crate) async fn next_event_except(
    rx: &mut broadcast::Receiver<Event>,
    skip: impl Fn(&Event) -> bool,
) -> Event {
    loop {
        let event = next_event(rx).await;
        if !skip(&event) {
            return event;
        }
    }
}

/// The status of `account` once it reports an error (polling every 20 ms,
/// at most `attempts` times); the last status seen otherwise.
pub(crate) async fn failed_status(
    service: &Service,
    account: &str,
    attempts: u32,
) -> Option<nail_api::AccountStatus> {
    let mut status = None;
    for _ in 0..attempts {
        if let Response::AccountStatuses { statuses } =
            service.handle(Request::ListAccountStatuses).await
        {
            status = statuses
                .into_iter()
                .find(|s| s.account_id.as_str() == account);
            if status.as_ref().is_some_and(|s| s.error.is_some()) {
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    status
}

/// A quiet service with `host` as the desktop shell (`None` = web mode).
pub(crate) fn desktop_service<H: nail_api::desktop::DesktopHost + 'static>(
    host: Option<Arc<H>>,
) -> (Service, TempDir, TempDir) {
    TempService::quiet_with(&[], |o| {
        o.desktop = host.map(|h| -> Arc<dyn nail_api::desktop::DesktopHost> { h });
    })
    .into_parts()
}

/// The plug-in has nothing (more) to place in the layout: an empty
/// proposal.
pub(crate) async fn assert_nothing_to_place(service: &Service, plugin: &nail_api::PluginId) {
    match service
        .handle(Request::PreviewPluginPlacements {
            plugin_id: plugin.clone(),
        })
        .await
    {
        Response::PluginPlacements {
            incoming, preview, ..
        } => {
            assert_eq!(incoming, "");
            assert!(preview.sections.is_empty(), "{:?}", preview.sections);
        }
        other => panic!("{other:?}"),
    }
}
