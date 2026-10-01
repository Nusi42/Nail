//! Nail's IMAP, POP3 and SMTP engines and the ManageSieve client against
//! real servers (plan M2b WP44, M2c): Dovecot (Pigeonhole) and Stalwart in
//! the CI job "interop". Every test returns at once without
//! `NAIL_INTEROP=1` (see `src/lib.rs` for the variables); the ManageSieve
//! tests run on servers with the `sieve` feature.

use std::time::Duration;

use nail_api::ErrorCode;
use nail_api::accounts::TlsMode;
use nail_api::engine::Envelope;
use nail_interop::{Interop, Server, from_env, message, unique};
use nail_proto_imap::{
    Credentials, Flag, ImapClient, ImapConfig, ImapError, Security, StoreMode,
    TlsTrust as ImapTrust,
};
use nail_proto_pop3::{Pop3Client, Pop3Config, Pop3Credentials, TlsTrust as Pop3Trust};
use nail_proto_smtp::{
    SendOptions, SmtpConfig, SmtpCredentials, SmtpSession, TlsTrust as SmtpTrust,
};

const TIMEOUT: Duration = Duration::from_secs(30);

fn imap_config(interop: &Interop, server: &Server, security: Security) -> ImapConfig {
    let port = match security {
        Security::ImplicitTls => server.imaps_port(),
        _ => server.imap_port(),
    };
    ImapConfig::new(&server.host)
        .with_port(port)
        .with_security(security)
        .with_tls_trust(ImapTrust::CustomRoots(interop.roots.clone()))
        .with_timeout(TIMEOUT)
}

async fn imap(interop: &Interop, server: &Server, security: Security) -> ImapClient {
    ImapClient::connect(
        &imap_config(interop, server, security),
        &Credentials::new(&interop.user, &interop.password),
    )
    .await
    .unwrap_or_else(|e| panic!("{} IMAP {security:?}: {e}", server.name))
}

fn pop3_config(interop: &Interop, server: &Server, tls: TlsMode) -> Pop3Config {
    let port = match tls {
        TlsMode::Implicit => server.pop3s_port(),
        _ => server.pop3_port(),
    };
    Pop3Config::new(&server.host)
        .with_port(port)
        .with_tls(tls)
        .with_trust(Pop3Trust::CustomRoots(interop.roots.clone()))
        .with_timeout(TIMEOUT)
}

fn pop3_credentials(interop: &Interop, password: &str) -> Pop3Credentials {
    Pop3Credentials::Password {
        username: interop.user.clone(),
        password: password.to_owned(),
        mechanism: None,
    }
}

fn smtp_config(interop: &Interop, server: &Server, tls: TlsMode) -> SmtpConfig {
    let port = match tls {
        TlsMode::Implicit => server.submissions_port(),
        _ => server.submission_port(),
    };
    SmtpConfig::new(&server.host)
        .with_port(port)
        .with_tls(tls)
        .with_trust(SmtpTrust::CustomRoots(interop.roots.clone()))
        .with_timeout(TIMEOUT)
}

fn smtp_credentials(interop: &Interop, password: &str) -> SmtpCredentials {
    SmtpCredentials::Password {
        username: interop.user.clone(),
        password: password.to_owned(),
        mechanism: None,
    }
}

/// Appends a fresh message to `folder`; its Message-ID.
async fn append_new(client: &mut ImapClient, interop: &Interop, folder: &str) -> String {
    let id = format!("{}@interop.nail", unique("msg"));
    let raw = message(&interop.user, &interop.user, &id, "Nail interop");
    client.append(folder, &raw, &[], None).await.unwrap();
    id
}

/// Removes the messages with this Message-ID from `folder`.
async fn remove(client: &mut ImapClient, folder: &str, message_id: &str) {
    let uids = client.find_by_message_id(folder, message_id).await.unwrap();
    if !uids.is_empty() {
        client
            .store_flags(folder, &uids, StoreMode::Add, &[Flag::Deleted], None)
            .await
            .unwrap();
        client.expunge_messages(folder, &uids).await.unwrap();
    }
}

#[tokio::test]
async fn imap_logs_in_with_tls_and_starttls() {
    let Some(interop) = from_env() else { return };
    for server in &interop.servers {
        for security in [Security::ImplicitTls, Security::StartTls] {
            let mut client = imap(&interop, server, security).await;
            let folders = client.list_folders().await.unwrap();
            assert!(
                folders.iter().any(|f| f.path.eq_ignore_ascii_case("INBOX")),
                "{}: {folders:?}",
                server.name
            );
            assert!(client.examine("INBOX").await.is_ok());
            client.logout().await.unwrap();
        }
    }
}

#[tokio::test]
async fn imap_rejects_a_wrong_password() {
    let Some(interop) = from_env() else { return };
    for server in &interop.servers {
        let err = ImapClient::connect(
            &imap_config(&interop, server, Security::ImplicitTls),
            &Credentials::new(&interop.user, "wrong password"),
        )
        .await
        .unwrap_err();
        assert!(
            matches!(err, ImapError::AuthFailed(_)),
            "{}: {err:?}",
            server.name
        );
        assert_eq!(err.code(), ErrorCode::AuthFailed);
    }
}

#[tokio::test]
async fn imap_folders_append_fetch_flags_move_expunge() {
    let Some(interop) = from_env() else { return };
    for server in &interop.servers {
        let mut client = imap(&interop, server, Security::ImplicitTls).await;
        let folder = unique("nail-interop");
        let archive = format!("{folder}-archive");
        client.create_folder(&folder).await.unwrap();
        client.create_folder(&archive).await.unwrap();

        let id = append_new(&mut client, &interop, &folder).await;
        let uids = client.find_by_message_id(&folder, &id).await.unwrap();
        assert_eq!(uids.len(), 1, "{}", server.name);

        client
            .store_flags(
                &folder,
                &uids,
                StoreMode::Add,
                &[Flag::Seen, Flag::Flagged],
                None,
            )
            .await
            .unwrap();
        client.examine(&folder).await.unwrap();
        let fetched = client.fetch_batch(&uids).await.unwrap();
        assert_eq!(fetched.len(), 1);
        let raw = String::from_utf8_lossy(&fetched[0].raw);
        assert!(raw.contains(&format!("<{id}>")), "{}: {raw}", server.name);
        assert!(fetched[0].flags.seen, "{}", server.name);
        assert!(fetched[0].flags.flagged, "{}", server.name);

        client
            .move_messages(&folder, &uids, &archive)
            .await
            .unwrap();
        assert!(
            client
                .find_by_message_id(&folder, &id)
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            client
                .find_by_message_id(&archive, &id)
                .await
                .unwrap()
                .len(),
            1,
            "{}: moved",
            server.name
        );

        remove(&mut client, &archive, &id).await;
        assert!(
            client
                .find_by_message_id(&archive, &id)
                .await
                .unwrap()
                .is_empty()
        );
        client.delete_folder(&archive).await.unwrap();
        client.delete_folder(&folder).await.unwrap();
        let folders = client.list_folders().await.unwrap();
        assert!(!folders.iter().any(|f| f.path == folder), "{}", server.name);
        client.logout().await.unwrap();
    }
}

#[tokio::test]
async fn pop3_reads_what_imap_stored() {
    let Some(interop) = from_env() else { return };
    for server in &interop.servers {
        let mut imap = imap(&interop, server, Security::ImplicitTls).await;
        let id = append_new(&mut imap, &interop, "INBOX").await;

        for tls in [TlsMode::Implicit, TlsMode::Starttls] {
            let mut pop3 = Pop3Client::connect(&pop3_config(&interop, server, tls))
                .await
                .unwrap_or_else(|e| panic!("{} POP3 {tls:?}: {e}", server.name));
            assert!(pop3.is_encrypted());
            pop3.login(&pop3_credentials(&interop, &interop.password))
                .await
                .unwrap_or_else(|e| panic!("{} POP3 {tls:?} login: {e}", server.name));
            let (count, _) = pop3.stat().await.unwrap();
            assert!(count > 0, "{}", server.name);
            let messages = pop3.messages().await.unwrap();
            let mut found = false;
            for m in &messages {
                let raw = pop3.retr(m.number).await.unwrap();
                if String::from_utf8_lossy(&raw).contains(&format!("<{id}>")) {
                    assert!(!m.uid.is_empty());
                    found = true;
                }
            }
            assert!(
                found,
                "{} POP3 {tls:?}: appended message not listed",
                server.name
            );
            pop3.quit().await.unwrap();
        }

        remove(&mut imap, "INBOX", &id).await;
        imap.logout().await.unwrap();
    }
}

#[tokio::test]
async fn pop3_rejects_a_wrong_password() {
    let Some(interop) = from_env() else { return };
    for server in &interop.servers {
        let mut pop3 = Pop3Client::connect(&pop3_config(&interop, server, TlsMode::Implicit))
            .await
            .unwrap();
        let err = pop3
            .login(&pop3_credentials(&interop, "wrong password"))
            .await
            .unwrap_err();
        assert_eq!(
            err.code(),
            ErrorCode::AuthFailed,
            "{}: {err:?}",
            server.name
        );
    }
}

#[tokio::test]
async fn smtp_submission_delivers_to_the_mailbox() {
    let Some(interop) = from_env() else { return };
    for server in interop.servers.iter().filter(|s| s.smtp) {
        for tls in [TlsMode::Starttls, TlsMode::Implicit] {
            let mut smtp = SmtpSession::connect(&smtp_config(&interop, server, tls))
                .await
                .unwrap_or_else(|e| panic!("{} SMTP {tls:?}: {e}", server.name));
            assert!(smtp.is_encrypted());
            smtp.authenticate(&smtp_credentials(&interop, &interop.password))
                .await
                .unwrap_or_else(|e| panic!("{} SMTP {tls:?} auth: {e}", server.name));
            let id = format!("{}@interop.nail", unique("sent"));
            let raw = message(&interop.user, &interop.user, &id, "Nail interop send");
            let envelope = Envelope {
                from: interop.user.clone(),
                to: vec![interop.user.clone()],
            };
            smtp.send(&envelope, &raw, SendOptions::default())
                .await
                .unwrap_or_else(|e| panic!("{} SMTP {tls:?} send: {e}", server.name));
            smtp.quit().await.unwrap();

            // Delivery is asynchronous: poll the inbox.
            let mut imap = imap(&interop, server, Security::ImplicitTls).await;
            let deadline = tokio::time::Instant::now() + TIMEOUT;
            loop {
                if !imap
                    .find_by_message_id("INBOX", &id)
                    .await
                    .unwrap()
                    .is_empty()
                {
                    break;
                }
                assert!(
                    tokio::time::Instant::now() < deadline,
                    "{} SMTP {tls:?}: not delivered",
                    server.name
                );
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            remove(&mut imap, "INBOX", &id).await;
            imap.logout().await.unwrap();
        }
    }
}

#[tokio::test]
async fn smtp_rejects_a_wrong_password() {
    let Some(interop) = from_env() else { return };
    for server in interop.servers.iter().filter(|s| s.smtp) {
        let mut smtp = SmtpSession::connect(&smtp_config(&interop, server, TlsMode::Implicit))
            .await
            .unwrap();
        let err = smtp
            .authenticate(&smtp_credentials(&interop, "wrong password"))
            .await
            .unwrap_err();
        assert_eq!(
            err.code(),
            ErrorCode::AuthFailed,
            "{}: {err:?}",
            server.name
        );
    }
}

// ---------------------------------------------------------------------------
// ManageSieve (M2c, ADR 0049)

fn sieve_config(interop: &Interop, server: &Server) -> nail_sieve::SieveConfig {
    nail_sieve::SieveConfig::new(&server.host)
        .with_port(server.sieve_port())
        .with_tls(TlsMode::Starttls)
        .with_trust(nail_sieve::TlsTrust::CustomRoots(interop.roots.clone()))
        .with_timeout(TIMEOUT)
}

async fn sieve(interop: &Interop, server: &Server) -> nail_sieve::SieveSession {
    nail_sieve::SieveSession::login(
        &sieve_config(interop, server),
        &nail_sieve::SieveCredentials::password(&interop.user, &interop.password),
    )
    .await
    .unwrap_or_else(|e| panic!("{} ManageSieve: {e}", server.name))
}

fn sieve_servers(interop: &Interop) -> Vec<Server> {
    interop
        .servers
        .iter()
        .filter(|s| s.sieve)
        .cloned()
        .collect()
}

/// A rule every server can run (`fileinto` + `imap4flags`, or kept by
/// the client where an extension is missing).
fn sieve_rules() -> Vec<nail_api::rules::Rule> {
    use nail_api::rules::{Condition, FolderRef, Rule, RuleAction, RuleTrigger, TextMatch, TextOp};
    vec![Rule {
        id: "interop".into(),
        name: "Interop".into(),
        enabled: true,
        triggers: vec![RuleTrigger::Arrival],
        scope: vec![],
        conditions: Condition::Subject {
            matches: TextMatch {
                op: TextOp::Contains,
                value: "[interop]".into(),
                case_sensitive: false,
            },
        },
        actions: vec![
            RuleAction::Move {
                folder: FolderRef::Path {
                    path: "Interop".into(),
                    account_id: None,
                },
            },
            RuleAction::Flag { flagged: true },
        ],
        source: None,
    }]
}

async fn remove_if_present(session: &mut nail_sieve::SieveSession, name: &str) {
    let scripts = session.list_scripts().await.unwrap();
    if let Some(script) = scripts.iter().find(|s| s.name == name) {
        if script.active {
            session.set_active("").await.unwrap();
        }
        session.delete_script(name).await.unwrap();
    }
}

#[tokio::test]
async fn managesieve_login_capabilities_and_checkscript() {
    let Some(interop) = from_env() else { return };
    for server in sieve_servers(&interop) {
        check_managesieve_basics(&interop, &server).await;
    }
}

#[tokio::test]
async fn managesieve_upload_keeps_the_users_script_and_switches_off() {
    let Some(interop) = from_env() else { return };
    for server in sieve_servers(&interop) {
        check_managesieve_upload(&interop, &server).await;
    }
}

/// The ManageSieve checks run offline too, against the in-process mock
/// server with STARTTLS, so they are known to work before CI reaches real
/// servers.
#[tokio::test]
async fn managesieve_checks_pass_against_the_mock_server() {
    let mock = nail_testkit_net::MockSieveServer::start(nail_testkit_net::MockSieveConfig {
        tls: nail_testkit::MockTls::StartTls,
        username: "ada@example.org".into(),
        password: "nail-interop".into(),
        ..nail_testkit_net::MockSieveConfig::default()
    })
    .await
    .unwrap();
    let interop = Interop {
        servers: vec![Server {
            name: "mock".into(),
            host: "127.0.0.1".into(),
            base: mock.port() - 4190,
            smtp: false,
            sieve: true,
        }],
        dav: Vec::new(),
        roots: vec![mock.ca_der().unwrap().to_vec()],
        user: "ada@example.org".into(),
        password: "nail-interop".into(),
    };
    let server = interop.servers[0].clone();
    check_managesieve_basics(&interop, &server).await;
    check_managesieve_upload(&interop, &server).await;
    assert!(mock.scripts().is_empty(), "{:?}", mock.scripts());
}

async fn check_managesieve_basics(interop: &Interop, server: &Server) {
    {
        let mut session = sieve(interop, server).await;
        assert!(session.is_tls(), "{}: STARTTLS", server.name);
        let caps = session.capabilities().clone();
        assert!(caps.has_extension("fileinto"), "{}: {caps:?}", server.name);
        let broken = session.check_script("if header :bogus { keep").await;
        if caps.version.is_some() {
            assert!(
                matches!(broken, Err(nail_sieve::SieveError::ScriptRejected(_))),
                "{}: {broken:?}",
                server.name
            );
        }
        session.logout().await.unwrap();

        let wrong = nail_sieve::SieveSession::login(
            &sieve_config(interop, server),
            &nail_sieve::SieveCredentials::password(&interop.user, "wrong"),
        )
        .await;
        assert!(
            matches!(wrong, Err(nail_sieve::SieveError::AuthFailed(_))),
            "{}: {wrong:?}",
            server.name
        );
    }
}

async fn check_managesieve_upload(interop: &Interop, server: &Server) {
    {
        let mut session = sieve(interop, server).await;
        let theirs = unique("user-rules");
        let (ours, renamed) = (unique("nail"), unique("nail-renamed"));
        session.put_script(&theirs, "keep;\r\n").await.unwrap();
        session.set_active(&theirs).await.unwrap();

        let folders = nail_sieve::FolderMap::new(Some('/'), &[]);
        let options = || nail_sieve::CompileOptions {
            account_id: "interop".into(),
            account_aliases: vec![],
            extensions: vec![],
            folders: &folders,
            own_addresses: vec![interop.user.clone()],
            utc_offset_minutes: 0,
            today: None,
            max_redirects: None,
            previous_active: None,
        };
        let vacation = nail_api::sieve::VacationSettings::default();
        let report = nail_sieve::upload(&mut session, &ours, &sieve_rules(), &vacation, options())
            .await
            .unwrap_or_else(|e| panic!("{}: upload: {e}", server.name));
        assert!(report.activated, "{}", server.name);
        assert_eq!(report.previous_active.as_deref(), Some(theirs.as_str()));
        let stored = session.get_script(&ours).await.unwrap();
        assert!(nail_sieve::is_managed(&stored), "{}: {stored}", server.name);
        assert_eq!(
            nail_sieve::recorded_previous(&stored).as_deref(),
            Some(theirs.as_str())
        );
        let back = nail_sieve::decompile(&stored).unwrap();
        assert!(back.managed);
        let active = session.list_scripts().await.unwrap();
        assert!(
            active.iter().any(|s| s.name == ours && s.active),
            "{active:?}"
        );

        // A new script name: Nail's old script is never the user's.
        let report =
            nail_sieve::upload(&mut session, &renamed, &sieve_rules(), &vacation, options())
                .await
                .unwrap();
        assert_eq!(report.replaced_managed.as_deref(), Some(ours.as_str()));
        assert_eq!(report.previous_active.as_deref(), Some(theirs.as_str()));
        let stored = session.get_script(&renamed).await.unwrap();
        assert!(
            !stored.contains(&format!("\"{ours}\"")),
            "{}: {stored}",
            server.name
        );
        remove_if_present(&mut session, &ours).await;

        // Switching off gives the user's script back.
        let restored = nail_sieve::remove(&mut session, &renamed).await.unwrap();
        assert_eq!(
            restored.as_deref(),
            Some(theirs.as_str()),
            "{}",
            server.name
        );
        let scripts = session.list_scripts().await.unwrap();
        assert!(
            scripts.iter().any(|s| s.name == theirs && s.active),
            "{scripts:?}"
        );
        assert!(!scripts.iter().any(|s| s.name == renamed));

        remove_if_present(&mut session, &theirs).await;
        session.logout().await.unwrap();
    }
}
