//! M2b integration in the core (ADR 0041): remote content through the
//! proxy, the admin requests routed to `nail-remote-auth`, requests from
//! remote sessions (authorization, grant narrowing) and the remote-UI
//! host link.

use std::sync::Arc;
use std::time::Duration;

use nail_api::engine::{MemorySecretStore, SecretStore};
use nail_api::mcp::McpScope;
use nail_api::remote::{
    AuthRequest, AuthResponse, DEVICE_COOKIE, RemoteAccessPreset, RemoteGrant, RemoteUiSettings,
    RemoteUiState, RemoteUiStatus, SESSION_COOKIE,
};
use nail_api::remote_content::{PROXY_URL_SCHEME, RemoteContentScope};
use nail_api::{
    AccountId, ErrorCode, MessageBody, MessageId, MessageSummary, Request, Response, UiContext,
};
use nail_core::demo::{DEMO_ACCOUNT_ID, demo_options, seed_demo};
use nail_core::{RemoteUiCommand, Service, ServiceOptions};
use nail_remote_auth::{ClientMeta, Cookies, RemoteSessionCtx};
use pretty_assertions::assert_eq;

struct Fixture {
    service: Service,
    _data: tempfile::TempDir,
    _config: tempfile::TempDir,
}

fn setup() -> Fixture {
    let data = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
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

async fn request(f: &Fixture, request: Request) -> Response {
    f.service.handle(request).await
}

fn error_code(response: &Response) -> Option<ErrorCode> {
    match response {
        Response::Error { code, .. } => Some(*code),
        _ => None,
    }
}

fn demo_messages(f: &Fixture) -> Vec<MessageSummary> {
    let store = f.service.store();
    let mut all = Vec::new();
    for folder in store
        .list_folders(&AccountId::new(DEMO_ACCOUNT_ID))
        .unwrap()
    {
        all.extend(store.list_messages(&folder.id, 0, 1000).unwrap().messages);
    }
    all
}

async fn body(f: &Fixture, id: &MessageId) -> MessageBody {
    match request(f, Request::GetMessage { id: id.clone() }).await {
        Response::Message { message } => *message,
        other => panic!("{other:?}"),
    }
}

/// The demo message with blocked remote content (the garden newsletter).
async fn remote_message(f: &Fixture) -> MessageSummary {
    for message in demo_messages(f) {
        if body(f, &message.id).await.remote_content.is_some() {
            return message;
        }
    }
    panic!("no demo message has remote content");
}

fn proxy_tokens(html: &str) -> Vec<String> {
    html.split(PROXY_URL_SCHEME)
        .skip(1)
        .map(|rest| {
            rest.chars()
                .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
                .collect()
        })
        .collect()
}

#[tokio::test]
async fn remote_content_is_blocked_until_allowed_then_proxied() {
    let f = setup();
    let message = remote_message(&f).await;
    let blocked = body(&f, &message.id).await;
    let info = blocked.remote_content.clone().unwrap();
    assert!(info.resources > 0);
    assert_eq!(info.allowed_by, None);
    let sender = message.from.clone().unwrap().email.to_lowercase();
    assert_eq!(info.sender.as_deref(), Some(sender.as_str()));
    assert!(blocked.remote_content_blocked);
    let html = blocked.sanitized_html.clone().unwrap();
    assert!(!html.contains(PROXY_URL_SCHEME));
    assert!(!html.contains("src=\"http"));

    let allowed = request(
        &f,
        Request::AllowRemoteContent {
            message_id: message.id.clone(),
            scope: RemoteContentScope::Sender,
        },
    )
    .await;
    assert!(
        matches!(allowed, Response::RemoteContentAllowed { .. }),
        "{allowed:?}"
    );
    let shown = body(&f, &message.id).await;
    let info = shown.remote_content.clone().unwrap();
    assert_eq!(info.allowed_by, Some(RemoteContentScope::Sender));
    assert!(!shown.remote_content_blocked);
    let html = shown.sanitized_html.unwrap();
    let tokens = proxy_tokens(&html);
    assert_eq!(tokens.len() as u32, info.resources, "{html}");
    assert!(!html.contains("src=\"http"));

    // Seeded runs never reach the internet: the proxy verifies the token,
    // asks the core, and cannot resolve the name.
    let proxy = f.service.remote_proxy().expect("a proxy in seeded runs");
    let err = proxy.serve(&tokens[0]).await.unwrap_err();
    assert_eq!(err.status(), 502, "{err}");
    // A forged token is a 404; nothing is fetched.
    assert_eq!(proxy.serve("abc.def").await.unwrap_err().status(), 404);

    // Removing the entry stops tokens already handed out.
    request(
        &f,
        Request::RemoveRemoteContentAllow {
            scope: RemoteContentScope::Sender,
            value: sender,
        },
    )
    .await;
    assert_eq!(proxy.serve(&tokens[0]).await.unwrap_err().status(), 403);
    let again = body(&f, &message.id).await;
    assert!(again.remote_content_blocked);
    assert_eq!(again.remote_content.unwrap().allowed_by, None);
}

#[tokio::test]
async fn stale_message_allows_are_purged_with_the_history() {
    let f = setup();
    let message = remote_message(&f).await;
    request(
        &f,
        Request::AllowRemoteContent {
            message_id: message.id.clone(),
            scope: RemoteContentScope::Message,
        },
    )
    .await;
    f.service
        .store()
        .allow_remote_content(RemoteContentScope::Message, "gone-message", None, 0)
        .unwrap();
    f.service.purge_history(2_000_000_000).await.unwrap();
    let left = f.service.store().list_remote_content_allows().unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].value, message.id.as_str());
}

/// A user with `grant` and a browser paired to them: the remote session.
async fn remote_session(f: &Fixture, name: &str, grant: RemoteGrant) -> RemoteSessionCtx {
    let user = match request(
        f,
        Request::CreateRemoteUser {
            name: name.into(),
            display_name: None,
            grant: grant.clone(),
        },
    )
    .await
    {
        Response::RemoteUserSaved { user } => user,
        other => panic!("{other:?}"),
    };
    let code = match request(
        f,
        Request::CreatePairingCode {
            user_id: user.id.clone(),
            device_name: Some("Phone".into()),
            grant: None,
            ttl_minutes: None,
        },
    )
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
async fn admin_requests_are_served_by_remote_auth() {
    let f = setup();
    let user = match request(
        &f,
        Request::CreateRemoteUser {
            name: "ada".into(),
            display_name: None,
            grant: RemoteAccessPreset::ReadOnly.grant(),
        },
    )
    .await
    {
        Response::RemoteUserSaved { user } => user,
        other => panic!("{other:?}"),
    };
    let saved = request(
        &f,
        Request::SetRemotePassword {
            user_id: user.id.clone(),
            password: Some("a long enough password".into()),
        },
    )
    .await;
    match saved {
        Response::RemoteUserSaved { user } => assert!(user.has_password),
        other => panic!("{other:?}"),
    }
    let short = request(
        &f,
        Request::SetRemotePassword {
            user_id: user.id.clone(),
            password: Some("short".into()),
        },
    )
    .await;
    assert_eq!(error_code(&short), Some(ErrorCode::InvalidRequest));
    let enrollment = request(
        &f,
        Request::StartTotpEnrollment {
            user_id: user.id.clone(),
        },
    )
    .await;
    assert!(
        matches!(enrollment, Response::TotpEnrollment { .. }),
        "{enrollment:?}"
    );
    match request(
        &f,
        Request::CreatePairingCode {
            user_id: user.id.clone(),
            device_name: None,
            grant: None,
            ttl_minutes: Some(5),
        },
    )
    .await
    {
        Response::PairingCodeCreated { code, url, .. } => {
            assert_eq!(code.len(), 9, "{code}");
            // Not running: no address to open yet.
            assert_eq!(url, None);
        }
        other => panic!("{other:?}"),
    }
    // Stored secrets are hashes only.
    let credentials = f.service.store().remote_credentials(&user.id).unwrap();
    assert!(credentials.password_hash.unwrap().starts_with("$argon2id$"));
}

#[tokio::test]
async fn remote_sessions_are_authorized_and_narrowed() {
    let f = setup();
    let ctx = remote_session(&f, "reader", RemoteAccessPreset::ReadOnly.grant()).await;
    let service = &f.service;
    let message = remote_message(&f).await;

    match service.handle_remote(&ctx, Request::ListAccounts).await {
        Response::Accounts { accounts } => assert!(!accounts.is_empty()),
        other => panic!("{other:?}"),
    }
    match service
        .handle_remote(
            &ctx,
            Request::GetMessage {
                id: message.id.clone(),
            },
        )
        .await
    {
        Response::Message { message } => {
            // No sandbox origin while the remote UI is not running.
            assert_eq!(message.render_url, None);
        }
        other => panic!("{other:?}"),
    }
    // No `organize` scope, no admin, and desktop-only requests.
    let flag = service
        .handle_remote(
            &ctx,
            Request::DeleteMessages {
                message_ids: vec![message.id.clone()],
                permanent: false,
            },
        )
        .await;
    assert_eq!(error_code(&flag), Some(ErrorCode::Forbidden));
    let users = service.handle_remote(&ctx, Request::ListRemoteUsers).await;
    assert_eq!(error_code(&users), Some(ErrorCode::Forbidden));
    let context = service
        .handle_remote(
            &ctx,
            Request::ReportUiContext {
                context: UiContext::default(),
            },
        )
        .await;
    assert_eq!(error_code(&context), Some(ErrorCode::Forbidden));
    // Self-service: the session's own sessions.
    let own = service
        .handle_remote(
            &ctx,
            Request::ListRemoteSessions {
                filter: nail_api::remote::RemoteSessionFilter {
                    user_id: Some(ctx.session.user_id.clone()),
                    ..Default::default()
                },
            },
        )
        .await;
    assert!(matches!(own, Response::RemoteSessions { .. }), "{own:?}");

    // A grant for another account only: the demo account disappears.
    let mut narrow = RemoteAccessPreset::ReadOnly.grant();
    narrow.mail.all_accounts = false;
    narrow.mail.accounts = vec![AccountId::new("somewhere-else")];
    let narrow = remote_session(&f, "narrow", narrow).await;
    match service.handle_remote(&narrow, Request::ListAccounts).await {
        Response::Accounts { accounts } => assert_eq!(accounts, vec![]),
        other => panic!("{other:?}"),
    }
    let read = service
        .handle_remote(
            &narrow,
            Request::GetMessage {
                id: message.id.clone(),
            },
        )
        .await;
    assert_eq!(error_code(&read), Some(ErrorCode::Forbidden));
    let folders = service
        .handle_remote(
            &narrow,
            Request::ListFolders {
                account_id: AccountId::new(DEMO_ACCOUNT_ID),
            },
        )
        .await;
    assert_eq!(error_code(&folders), Some(ErrorCode::Forbidden));

    // Every denial is in the security log.
    let audit = service.store().list_remote_audit(None, None, 100).unwrap();
    assert!(
        audit
            .iter()
            .filter(|e| e.outcome == nail_api::remote::RemoteAuditOutcome::Denied)
            .count()
            >= 3
    );
    assert!(ctx.grant().mail.has(McpScope::Read));
}

#[tokio::test]
async fn admin_sessions_change_credentials_as_themselves() {
    let f = setup();
    let mut grant = RemoteAccessPreset::Full.grant();
    grant.admin = true;
    let admin = remote_session(&f, "admin", grant).await;
    // Paired just now: counts as a fresh login.
    let users = f
        .service
        .handle_remote(&admin, Request::ListRemoteUsers)
        .await;
    assert!(matches!(users, Response::RemoteUsers { .. }), "{users:?}");
    let saved = f
        .service
        .handle_remote(
            &admin,
            Request::SetRemotePassword {
                user_id: admin.session.user_id.clone(),
                password: Some("another long password".into()),
            },
        )
        .await;
    assert!(
        matches!(saved, Response::RemoteUserSaved { .. }),
        "{saved:?}"
    );
    // The actor's own session survives its password change.
    assert!(f.service.remote_auth().refresh(&admin).is_some());
}

#[tokio::test]
async fn the_remote_ui_host_receives_settings_and_reports_status() {
    let f = setup();
    // Without a host: stored, stopped; no certificate to make.
    let regenerate = request(&f, Request::RegenerateRemoteCertificate).await;
    assert_eq!(error_code(&regenerate), Some(ErrorCode::Unsupported));

    let mut commands = f.service.attach_remote_ui();
    let mut events = f.service.subscribe();
    let settings = RemoteUiSettings {
        enabled: true,
        host_name: Some("nail.local".into()),
        ..RemoteUiSettings::default()
    };
    match request(
        &f,
        Request::UpdateRemoteUiSettings {
            settings: settings.clone(),
        },
    )
    .await
    {
        Response::RemoteUi { status, .. } => assert_eq!(status.state, RemoteUiState::Starting),
        other => panic!("{other:?}"),
    }
    match commands.recv().await.unwrap() {
        RemoteUiCommand::Apply(applied) => assert_eq!(*applied, settings),
        other => panic!("{other:?}"),
    }
    let mut running = RemoteUiStatus::stopped();
    running.state = RemoteUiState::Running;
    running.urls = vec!["https://nail.local:4780/".into()];
    f.service
        .report_remote_ui(Some(running), Some("https://nail.local:50000".into()));
    let changed = loop {
        match tokio::time::timeout(Duration::from_secs(5), events.recv())
            .await
            .unwrap()
            .unwrap()
        {
            nail_api::Event::RemoteUiStatusChanged { status }
                if status.state != RemoteUiState::Starting =>
            {
                break status;
            }
            _ => continue,
        }
    };
    assert_eq!(changed.state, RemoteUiState::Running);

    // Remote readers now get the sandbox document's address.
    let ctx = remote_session(&f, "reader", RemoteAccessPreset::ReadOnly.grant()).await;
    let message = remote_message(&f).await;
    match f
        .service
        .handle_remote(
            &ctx,
            Request::GetMessage {
                id: message.id.clone(),
            },
        )
        .await
    {
        Response::Message { message } => {
            let url = message.render_url.unwrap();
            assert!(url.starts_with("https://nail.local:50000/render/"), "{url}");
        }
        other => panic!("{other:?}"),
    }
    // The desktop's own reader keeps rendering the HTML itself.
    assert_eq!(body(&f, &message.id).await.render_url, None);

    // Certificate regeneration is the host's.
    let service = f.service.clone();
    let asked =
        tokio::spawn(async move { service.handle(Request::RegenerateRemoteCertificate).await });
    match commands.recv().await.unwrap() {
        RemoteUiCommand::RegenerateCertificate(reply) => reply.send(Ok(())).unwrap(),
        other => panic!("{other:?}"),
    }
    assert!(matches!(asked.await.unwrap(), Response::RemoteUi { .. }));

    // Internet mode stays locked.
    let internet = request(
        &f,
        Request::UpdateRemoteUiSettings {
            settings: RemoteUiSettings {
                internet_mode: true,
                ..settings
            },
        },
    )
    .await;
    assert_eq!(error_code(&internet), Some(ErrorCode::Forbidden));
}

#[tokio::test]
async fn proxy_keys_live_in_the_keychain_and_survive_restarts() {
    let data = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::new());
    let open = || {
        let mut options = ServiceOptions::new(data.path(), config.path());
        options.watch_config = false;
        options.start_engines = false;
        options.secrets = Some(Arc::clone(&secrets));
        Service::open(demo_options(options)).unwrap()
    };
    let first = open();
    seed_demo(first.store(), first.blobs()).unwrap();
    let signer_token = first
        .remote_proxy()
        .unwrap()
        .signer()
        .sign_resource("m-1", "https://images.example.org/a.png")
        .unwrap();
    drop(first);
    let second = open();
    let grant = second
        .remote_proxy()
        .unwrap()
        .signer()
        .verify(&signer_token, nail_proxy::Purpose::Resource)
        .unwrap();
    assert_eq!(grant.message_id, "m-1");
}
