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

// ---------------------------------------------------------------------------
// Review fixes (M2b): every request is checked against the grant, drafts
// and searches respect the `encrypted` gate, events are filtered.
// ---------------------------------------------------------------------------

fn grant_with(preset: RemoteAccessPreset, extra: &[McpScope]) -> RemoteGrant {
    let mut grant = preset.grant();
    grant.mail.scopes.extend_from_slice(extra);
    grant
}

/// The Full preset, but for another account only.
fn elsewhere(extra: &[McpScope]) -> RemoteGrant {
    let mut grant = grant_with(RemoteAccessPreset::Full, extra);
    grant.mail.all_accounts = false;
    grant.mail.accounts = vec![AccountId::new("somewhere-else")];
    grant
}

/// Marks a message as end-to-end encrypted (what the reader records after
/// decrypting it).
fn mark_encrypted(f: &Fixture, id: &MessageId) {
    let status: nail_api::security::SecurityStatus = serde_json::from_value(serde_json::json!({
        "technology": "openpgp",
        "encryption": { "state": "decrypted" },
    }))
    .unwrap();
    f.service
        .store()
        .put_message_security(&nail_store::MessageSecurityRecord {
            message_id: id.clone(),
            status: Some(status),
            auth_results: None,
            checked_at: 1,
        })
        .unwrap();
}

/// The demo data with an `accounts.kdl` naming the demo account, so it
/// has an identity to compose with (no servers are contacted).
fn setup_with_identity() -> Fixture {
    let data = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    std::fs::write(
        config.path().join("accounts.kdl"),
        format!(
            "account \"{DEMO_ACCOUNT_ID}\" email=\"ada@demo.example\" name=\"Demo\" {{
                 imap host=\"127.0.0.1\" port=1 tls=\"plaintext\"
                 smtp host=\"127.0.0.1\" port=1 tls=\"plaintext\"
                 identity \"ada@demo.example\" name=\"Ada\"
}}
"
        ),
    )
    .unwrap();
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

async fn demo_identity(f: &Fixture) -> nail_api::IdentityId {
    match request(f, Request::ListIdentities { account_id: None }).await {
        Response::Identities { identities } => identities[0].id.clone(),
        other => panic!("{other:?}"),
    }
}

fn reply_draft(identity: nail_api::IdentityId, original: &MessageId) -> nail_api::Draft {
    nail_api::Draft {
        id: None,
        identity_id: identity,
        to: vec![nail_api::Address {
            name: None,
            email: "someone@example.org".into(),
        }],
        cc: vec![],
        bcc: vec![],
        subject: "Re: secret".into(),
        mode: nail_api::accounts::ComposerMode::Plain,
        text: "> the decrypted text".into(),
        html: None,
        markdown: None,
        attachments: vec![],
        context: Some(nail_api::DraftContext {
            mode: nail_api::DraftContextMode::Reply,
            message_id: original.clone(),
        }),
        signature: nail_api::SignatureChoice::Identity,
        markdown_part: false,
        automatic: None,
        crypto: None,
    }
}

fn new_draft(mode: nail_api::DraftContextMode, message_id: &MessageId) -> Request {
    Request::NewDraft {
        identity_id: None,
        account_id: None,
        mode: None,
        context: Some(nail_api::DraftContext {
            mode,
            message_id: message_id.clone(),
        }),
    }
}

#[tokio::test]
async fn drafts_and_the_composer_respect_the_encrypted_gate() {
    let f = setup_with_identity();
    let service = &f.service;
    let messages = demo_messages(&f);
    let secret = messages[0].id.clone();
    mark_encrypted(&f, &secret);
    let identity = demo_identity(&f).await;
    // A reply draft made on the desktop quotes the decrypted text.
    let Response::DraftSaved { draft_id } = request(
        &f,
        Request::SaveDraft {
            draft: reply_draft(identity.clone(), &secret),
        },
    )
    .await
    else {
        panic!("draft not saved")
    };

    let plain = remote_session(&f, "plain", grant_with(RemoteAccessPreset::Full, &[])).await;
    // Forwarding (or replying to) the encrypted message would copy its
    // decrypted body and attachments into the answer.
    for mode in [
        nail_api::DraftContextMode::Forward,
        nail_api::DraftContextMode::Reply,
    ] {
        let forward = service
            .handle_remote(&plain, new_draft(mode, &secret))
            .await;
        assert_eq!(
            error_code(&forward),
            Some(ErrorCode::Forbidden),
            "{forward:?}"
        );
    }
    // The desktop's reply draft is hidden, and cannot be read or sent.
    match service.handle_remote(&plain, Request::ListDrafts).await {
        Response::Drafts { drafts } => {
            assert!(drafts.iter().all(|d| d.id.as_ref() != Some(&draft_id)))
        }
        other => panic!("{other:?}"),
    }
    let mut redirect = reply_draft(identity.clone(), &secret);
    redirect.context = None;
    redirect.automatic = Some(nail_api::AutomaticMail::Redirect {
        message_id: secret.clone(),
    });
    for denied in [
        Request::GetDraft {
            draft_id: draft_id.clone(),
        },
        Request::SendDraft {
            draft_id: draft_id.clone(),
            delay_secs: None,
            send_at: None,
            crypto: None,
        },
        Request::SaveDraft {
            draft: reply_draft(identity.clone(), &secret),
        },
        // A redirect sends the stored message itself.
        Request::SaveDraft { draft: redirect },
    ] {
        let name = format!("{denied:?}");
        let response = service.handle_remote(&plain, denied).await;
        assert_eq!(
            error_code(&response),
            Some(ErrorCode::Forbidden),
            "{name}: {response:?}"
        );
    }

    // Replying to a plain message still works.
    let reply = service
        .handle_remote(
            &plain,
            new_draft(nail_api::DraftContextMode::Reply, &messages[1].id),
        )
        .await;
    assert!(matches!(reply, Response::Draft { .. }), "{reply:?}");

    // With the `encrypted` scope the draft is there.
    let trusted = remote_session(
        &f,
        "trusted",
        grant_with(RemoteAccessPreset::Full, &[McpScope::Encrypted]),
    )
    .await;
    match service.handle_remote(&trusted, Request::ListDrafts).await {
        Response::Drafts { drafts } => {
            assert!(drafts.iter().any(|d| d.id.as_ref() == Some(&draft_id)))
        }
        other => panic!("{other:?}"),
    }
    let read = service
        .handle_remote(&trusted, Request::GetDraft { draft_id })
        .await;
    assert!(matches!(read, Response::Draft { .. }), "{read:?}");
}

#[tokio::test]
async fn requests_naming_data_outside_the_grant_are_refused() {
    let f = setup_with_identity();
    let service = &f.service;
    let messages = demo_messages(&f);
    let message = messages[0].clone();
    let identity = demo_identity(&f).await;
    // Desktop state the narrow session must not see.
    let Response::DraftSaved { draft_id } = request(
        &f,
        Request::SaveDraft {
            draft: reply_draft(identity.clone(), &message.id),
        },
    )
    .await
    else {
        panic!("draft not saved")
    };
    request(
        &f,
        Request::AllowRemoteContent {
            message_id: message.id.clone(),
            scope: RemoteContentScope::Message,
        },
    )
    .await;

    let narrow = remote_session(&f, "narrow", elsewhere(&[McpScope::Encrypted])).await;
    let mut own_draft = reply_draft(identity.clone(), &message.id);
    own_draft.context = None;
    let mut forbidden = vec![
        // A draft forwarding a message of another account.
        new_draft(nail_api::DraftContextMode::Forward, &message.id),
        // Composing as another account's identity.
        Request::NewDraft {
            identity_id: Some(identity.clone()),
            account_id: None,
            mode: None,
            context: None,
        },
        Request::SaveDraft { draft: own_draft },
        Request::GetDraft {
            draft_id: draft_id.clone(),
        },
        Request::DeleteDraft {
            draft_id: draft_id.clone(),
        },
        Request::SendDraft {
            draft_id: draft_id.clone(),
            delay_secs: None,
            send_at: None,
            crypto: None,
        },
        Request::CheckRecipientKeys {
            identity_id: identity.clone(),
            recipients: vec!["someone@example.org".into()],
            lookup: false,
        },
        // Rules span every account; runs on named messages are checked.
        Request::ListRules,
        Request::RunRule {
            rule_id: nail_api::RuleId::from("any"),
            message_ids: vec![message.id.clone()],
            folder_id: None,
        },
        Request::RunRule {
            rule_id: nail_api::RuleId::from("any"),
            message_ids: vec![],
            folder_id: None,
        },
        // The folder belongs to an account outside the grant.
        Request::ListMessages {
            folder_id: message.folder_id.clone(),
            offset: 0,
            limit: 10,
        },
        Request::RemoveRemoteContentAllow {
            scope: RemoteContentScope::Message,
            value: message.id.as_str().to_owned(),
        },
    ];
    if let Some(thread_id) = message.thread_id.clone() {
        forbidden.push(Request::GetThread { thread_id });
    }
    for request in forbidden {
        let name = format!("{request:?}");
        let response = service.handle_remote(&narrow, request).await;
        assert_eq!(
            error_code(&response),
            Some(ErrorCode::Forbidden),
            "{name}: {response:?}"
        );
    }
    // Lists leave the other account's entries out.
    match service.handle_remote(&narrow, Request::ListDrafts).await {
        Response::Drafts { drafts } => assert_eq!(drafts, vec![]),
        other => panic!("{other:?}"),
    }
    match service.handle_remote(&narrow, Request::ListOutbox).await {
        Response::Outbox { entries } => assert_eq!(entries, vec![]),
        other => panic!("{other:?}"),
    }
    match service
        .handle_remote(&narrow, Request::ListRemoteContentAllows)
        .await
    {
        Response::RemoteContentAllows { entries } => assert!(
            entries
                .iter()
                .all(|e| e.scope != RemoteContentScope::Message)
        ),
        other => panic!("{other:?}"),
    }
    match service
        .handle_remote(&narrow, Request::ListIdentities { account_id: None })
        .await
    {
        Response::Identities { identities } => assert_eq!(identities, vec![]),
        other => panic!("{other:?}"),
    }
    // Collected addresses are not per account.
    match service
        .handle_remote(
            &narrow,
            Request::SuggestAddresses {
                prefix: String::new(),
                limit: 10,
            },
        )
        .await
    {
        Response::AddressSuggestions { addresses } => assert_eq!(addresses, vec![]),
        other => panic!("{other:?}"),
    }
    // The UI context names nothing outside the grant.
    request(
        &f,
        Request::ReportUiContext {
            context: UiContext {
                account_id: Some(message.account_id.clone()),
                folder_id: Some(message.folder_id.clone()),
                message_id: Some(message.id.clone()),
                selected_text: Some("private words".into()),
                selected_message_ids: vec![message.id.clone()],
                draft_id: Some(draft_id.clone()),
                ..UiContext::default()
            },
        },
    )
    .await;
    let watcher = remote_session(
        &f,
        "watcher",
        elsewhere(&[McpScope::Context, McpScope::Encrypted]),
    )
    .await;
    match service.handle_remote(&watcher, Request::GetUiContext).await {
        Response::UiContext { context } => assert_eq!(context, UiContext::default()),
        other => panic!("{other:?}"),
    }

    // The same requests with a full grant go through.
    let full = remote_session(
        &f,
        "full",
        grant_with(
            RemoteAccessPreset::Full,
            &[McpScope::Encrypted, McpScope::Context],
        ),
    )
    .await;
    match service.handle_remote(&full, Request::GetUiContext).await {
        Response::UiContext { context } => {
            assert_eq!(context.message_id, Some(message.id.clone()));
            assert_eq!(context.selected_text.as_deref(), Some("private words"));
            assert_eq!(context.draft_id, Some(draft_id.clone()));
        }
        other => panic!("{other:?}"),
    }
    let get = service
        .handle_remote(&full, Request::GetDraft { draft_id })
        .await;
    assert!(matches!(get, Response::Draft { .. }), "{get:?}");
    let rules = service.handle_remote(&full, Request::ListRules).await;
    assert!(matches!(rules, Response::Rules { .. }), "{rules:?}");
}

#[tokio::test]
async fn remote_search_hides_encrypted_mail_while_the_index_holds_decrypted_text() {
    let f = setup();
    let service = &f.service;
    service.rebuild_search_index().unwrap();
    let messages = demo_messages(&f);
    let secret = messages[0].clone();
    mark_encrypted(&f, &secret.id);
    let query = |text: &str| Request::Search {
        query: nail_api::SearchQuery {
            text: text.into(),
            account_id: None,
            folder_id: None,
            offset: 0,
            limit: 50,
            sort: Default::default(),
        },
    };
    let ids = |response: Response| match response {
        Response::SearchResults { hits, total, .. } => {
            assert!(total as usize >= hits.len());
            hits.into_iter().map(|h| h.message.id).collect::<Vec<_>>()
        }
        other => panic!("{other:?}"),
    };
    let text = format!("from:{}", secret.from.clone().unwrap().email);
    let plain = remote_session(&f, "plain", RemoteAccessPreset::ReadOnly.grant()).await;
    let trusted = remote_session(
        &f,
        "trusted",
        grant_with(RemoteAccessPreset::ReadOnly, &[McpScope::Encrypted]),
    )
    .await;
    // Decrypted indexing off: the index holds no decrypted text.
    assert!(ids(service.handle_remote(&plain, query(&text)).await).contains(&secret.id));

    f.service
        .store()
        .set_setting(
            nail_store::SECURITY_SETTINGS_KEY,
            &nail_api::security::SecuritySettings {
                index_decrypted: vec![AccountId::new(DEMO_ACCOUNT_ID)],
                ..Default::default()
            },
        )
        .unwrap();
    let hidden = ids(service.handle_remote(&plain, query(&text)).await);
    assert!(!hidden.contains(&secret.id), "{hidden:?}");
    assert!(ids(service.handle_remote(&trusted, query(&text)).await).contains(&secret.id));

    // A narrowed grant pages over what it may see only.
    let narrow = remote_session(&f, "narrow", elsewhere(&[])).await;
    match service.handle_remote(&narrow, query("")).await {
        Response::SearchResults { total, hits, .. } => {
            assert_eq!((total, hits.len()), (0, 0));
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn remote_sockets_get_only_the_events_their_grant_covers() {
    let f = setup();
    let service = &f.service;
    let message = demo_messages(&f)[0].clone();
    let reader = remote_session(&f, "reader", RemoteAccessPreset::ReadOnly.grant()).await;
    let narrow = remote_session(&f, "narrow", elsewhere(&[])).await;
    let mut admin_grant = RemoteAccessPreset::Full.grant();
    admin_grant.admin = true;
    let admin = remote_session(&f, "admin", admin_grant).await;

    let alert = nail_api::Event::RemoteSecurityAlert {
        alert: nail_api::remote::RemoteAlert::Lockout {
            user_name: Some("ada".into()),
            ip: Some("192.168.1.9".into()),
            until: "2026-01-01T00:00:00Z".into(),
        },
    };
    let status = nail_api::Event::RemoteUiStatusChanged {
        status: RemoteUiStatus::stopped(),
    };
    for admin_only in [
        alert,
        status,
        nail_api::Event::RemoteAccessChanged,
        nail_api::Event::McpClientsChanged,
    ] {
        assert_eq!(
            service.remote_event(&reader, admin_only.clone()).await,
            None
        );
        assert_eq!(
            service.remote_event(&admin, admin_only.clone()).await,
            Some(admin_only)
        );
    }
    let new_mail = nail_api::Event::NewMail {
        account_id: message.account_id.clone(),
        folder_id: message.folder_id.clone(),
        message_ids: vec![message.id.clone()],
    };
    let changed = nail_api::Event::MailChanged {
        folder_ids: vec![message.folder_id.clone()],
    };
    let failed = nail_api::Event::SyncFailed {
        account_id: message.account_id.clone(),
        code: ErrorCode::Network,
        message: "connection refused".into(),
        localized: None,
    };
    for mail in [new_mail, changed, failed] {
        assert_eq!(service.remote_event(&narrow, mail.clone()).await, None);
        assert_eq!(
            service.remote_event(&reader, mail.clone()).await,
            Some(mail)
        );
    }
    // Only the session's own end.
    let ended = |session_id| nail_api::Event::RemoteSessionEnded {
        session_id,
        reason: nail_api::remote::SessionEndReason::Revoked,
    };
    assert_eq!(
        service
            .remote_event(&reader, ended(narrow.session.id.clone()))
            .await,
        None
    );
    let own = ended(reader.session.id.clone());
    assert_eq!(service.remote_event(&reader, own.clone()).await, Some(own));
    assert_eq!(
        service
            .remote_event(&reader, nail_api::Event::LaunchRequested)
            .await,
        None
    );
}

/// M2c (ADR 0043): the new requests are checked against the grant like the
/// others: rule placements need every account, messages must be granted,
/// settings are administration, and a narrowed session sees only its own
/// notification overrides.
#[tokio::test]
async fn m2c_requests_respect_the_grant() {
    let f = setup();
    let service = &f.service;
    let message = demo_messages(&f)[0].id.clone();
    let full = remote_session(&f, "full", grant_with(RemoteAccessPreset::Full, &[])).await;
    let narrow = remote_session(&f, "narrow", elsewhere(&[])).await;

    // Rule placements span every account.
    let status = Request::GetSieveStatus { account_id: None };
    assert!(matches!(
        service.handle_remote(&full, status.clone()).await,
        Response::SieveStatus { .. }
    ));
    assert_eq!(
        error_code(&service.handle_remote(&narrow, status).await),
        Some(ErrorCode::Forbidden)
    );
    // Messages outside the grant.
    for request in [
        Request::FetchMessageBodies {
            message_ids: vec![message.clone()],
        },
        Request::RuleFromMessage {
            message_id: message.clone(),
        },
        Request::MarkJunk {
            message_ids: vec![message.clone()],
            junk: true,
        },
    ] {
        assert_eq!(
            error_code(&service.handle_remote(&narrow, request).await),
            Some(ErrorCode::Forbidden)
        );
    }
    assert!(matches!(
        service
            .handle_remote(
                &full,
                Request::FetchMessageBodies {
                    message_ids: vec![message.clone()],
                },
            )
            .await,
        Response::BodiesFetched { .. }
    ));
    // Accounts outside the grant.
    assert_eq!(
        error_code(
            &service
                .handle_remote(
                    &narrow,
                    Request::GetJunkStatus {
                        account_id: DEMO_ACCOUNT_ID.into(),
                    },
                )
                .await
        ),
        Some(ErrorCode::Forbidden)
    );
    // Administration needs `admin` (and a fresh login).
    for request in [
        Request::GetMcpSettings,
        Request::UpdateNotificationSettings {
            settings: Default::default(),
        },
    ] {
        assert!(error_code(&service.handle_remote(&full, request).await).is_some());
    }
    // Notification overrides are narrowed to the grant.
    let settings = nail_api::triage::NotificationSettings {
        overrides: vec![nail_api::triage::NotificationOverride {
            scope: nail_api::triage::NotificationScope::Account {
                account_id: DEMO_ACCOUNT_ID.into(),
            },
            notify: false,
        }],
        ..Default::default()
    };
    assert!(matches!(
        request(&f, Request::UpdateNotificationSettings { settings }).await,
        Response::NotificationSettings { .. }
    ));
    match service
        .handle_remote(&narrow, Request::GetNotificationSettings)
        .await
    {
        Response::NotificationSettings { settings } => assert!(settings.overrides.is_empty()),
        other => panic!("{other:?}"),
    }
    match service
        .handle_remote(&full, Request::GetNotificationSettings)
        .await
    {
        Response::NotificationSettings { settings } => assert_eq!(settings.overrides.len(), 1),
        other => panic!("{other:?}"),
    }
}
