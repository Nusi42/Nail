//! The ManageSieve client and the managed-script upload against the mock
//! server (`nail_testkit_net::MockSieveServer`).
#![cfg(test)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use nail_api::ErrorCode;
use nail_api::accounts::{AuthConfig, SaslMechanism, SecretRef, TlsMode};
use nail_api::domain::FolderRole;
use nail_api::engine::{
    AccessTokenProvider, BoxFuture, EngineError, MemorySecretStore, SecretStore,
};
use nail_api::rules::{Condition, FolderRef, Rule, RuleAction, RuleTrigger, TextMatch, TextOp};
use nail_api::sieve::{RulePlacement, VacationSettings};
use nail_sieve::{
    CompileOptions, FolderMap, MANAGED_MARKER, PREVIOUS_MARKER, SieveAuth, SieveConfig,
    SieveCredentials, SieveError, SieveSession, TlsTrust, decompile, reasons, remove, to_crlf,
    upload,
};
use nail_testkit::MockTls;
use nail_testkit_net::{MockPop3Config, MockPop3Server, MockSieveConfig, MockSieveServer};

const USER: &str = "sam@nail.example";
const PASSWORD: &str = "secret";

fn config(server: &MockSieveServer, tls: TlsMode) -> SieveConfig {
    let mut config = SieveConfig::new("127.0.0.1")
        .with_port(server.port())
        .with_tls(tls)
        .with_timeout(Duration::from_secs(5));
    if let Some(ca) = server.ca_der() {
        config = config.with_trust(TlsTrust::CustomRoots(vec![ca.to_vec()]));
    }
    config
}

fn password() -> SieveCredentials {
    SieveCredentials::password(USER, PASSWORD)
}

async fn plain_server(config: MockSieveConfig) -> (MockSieveServer, SieveSession) {
    let server = MockSieveServer::start(config).await.unwrap();
    let session = SieveSession::login(&self::config(&server, TlsMode::Plaintext), &password())
        .await
        .unwrap();
    (server, session)
}

#[tokio::test]
async fn script_commands() {
    let (server, mut session) = plain_server(MockSieveConfig::default()).await;
    let caps = session.capabilities().clone();
    assert!(caps.has_extension("fileinto") && caps.has_extension("VACATION"));
    assert_eq!(caps.max_redirects, Some(4));
    assert_eq!(caps.version.as_deref(), Some("1.0"));
    assert_eq!(caps.sasl, ["PLAIN", "XOAUTH2", "OAUTHBEARER"]);
    assert!(!session.is_tls());

    assert_eq!(session.list_scripts().await.unwrap().len(), 0);
    let script = "require \"fileinto\";\nif size :over 1M {\n  fileinto \"Big\";\n}\n";
    assert_eq!(session.check_script(script).await.unwrap(), Some(vec![]));
    assert!(
        session
            .put_script("mine \"quoted\"", script)
            .await
            .unwrap()
            .is_empty(),
        "{:?}",
        session.put_script("mine \"quoted\"", script).await.unwrap()
    );
    session.set_active("mine \"quoted\"").await.unwrap();
    let listed = session.list_scripts().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert!(listed[0].active && listed[0].name == "mine \"quoted\"");
    // Stored with CRLF line ends; read back unchanged.
    assert_eq!(
        session.get_script("mine \"quoted\"").await.unwrap(),
        to_crlf(script)
    );
    assert_eq!(server.active().as_deref(), Some("mine \"quoted\""));
    session.have_space("x", 100).await.unwrap();

    // The active script cannot be deleted; errors carry the response code.
    match session.delete_script("mine \"quoted\"").await {
        Err(SieveError::Refused { command, code, .. }) => {
            assert_eq!(command, "DELETESCRIPT");
            assert_eq!(code.as_deref(), Some("ACTIVE"));
        }
        other => panic!("{other:?}"),
    }
    session.set_active("").await.unwrap();
    session.delete_script("mine \"quoted\"").await.unwrap();
    assert!(matches!(
        session.get_script("gone").await,
        Err(SieveError::Refused { .. })
    ));

    // A script the server rejects: its complaint reaches the user.
    let err = session.check_script("fileinto \"x\";").await.unwrap_err();
    assert_eq!(err.code(), ErrorCode::Rejected);
    let text = err.localized();
    assert_eq!(text.id, reasons::ERROR_SCRIPT);
    assert!(text.args["detail"].to_string().contains("without require"));
    assert!(matches!(
        session.put_script("bad", "keep").await,
        Err(SieveError::ScriptRejected(_))
    ));
    session.logout().await.unwrap();
    assert_eq!(server.logins(), 1);
    assert!(server.commands().contains(&"AUTHENTICATE PLAIN".to_owned()));
}

#[tokio::test]
async fn quota_faults_and_disconnects() {
    let (server, mut session) = plain_server(MockSieveConfig {
        max_script_size: Some(10),
        ..MockSieveConfig::default()
    })
    .await;
    let err = session
        .put_script("a", "keep; keep; keep;")
        .await
        .unwrap_err();
    assert!(matches!(err, SieveError::Quota(_)), "{err:?}");
    assert_eq!(err.code(), ErrorCode::TooLarge);
    assert!(matches!(
        session.have_space("a", 100).await,
        Err(SieveError::Quota(_))
    ));
    server.fail("LISTSCRIPTS", "database down", 1);
    assert!(matches!(
        session.list_scripts().await,
        Err(SieveError::Refused { message, .. }) if message == "database down"
    ));
    server.disconnect_on("SETACTIVE");
    let err = session.set_active("a").await.unwrap_err();
    assert!(matches!(err, SieveError::ConnectionLost(_)), "{err:?}");
    assert!(err.is_retryable());
}

#[tokio::test]
async fn starttls_and_implicit_tls() {
    let server = MockSieveServer::start(MockSieveConfig {
        tls: MockTls::StartTls,
        ..MockSieveConfig::default()
    })
    .await
    .unwrap();
    let mut session = SieveSession::login(&config(&server, TlsMode::Starttls), &password())
        .await
        .unwrap();
    assert!(session.is_tls());
    // SASL was hidden before STARTTLS; the re-issued capabilities show it.
    assert!(session.capabilities().sasl.contains(&"PLAIN".to_owned()));
    session.put_script("a", "keep;").await.unwrap();
    assert!(server.commands().contains(&"STARTTLS".to_owned()));

    // Plaintext against a STARTTLS server: the server refuses the login.
    let err = SieveSession::login(&config(&server, TlsMode::Plaintext), &password())
        .await
        .unwrap_err();
    assert!(matches!(err, SieveError::EncryptionNeeded(_)), "{err:?}");

    let implicit = MockSieveServer::start(MockSieveConfig {
        tls: MockTls::Implicit,
        ..MockSieveConfig::default()
    })
    .await
    .unwrap();
    let session = SieveSession::login(&config(&implicit, TlsMode::Implicit), &password())
        .await
        .unwrap();
    assert!(session.is_tls());
    session.logout().await.unwrap();

    // An untrusted certificate fails the handshake.
    let untrusted = config(&implicit, TlsMode::Implicit).with_trust(TlsTrust::CustomRoots(vec![]));
    let err = SieveSession::connect(&untrusted).await.unwrap_err();
    assert!(matches!(err, SieveError::Tls { .. }), "{err:?}");
    assert_eq!(err.code(), ErrorCode::Tls);
}

#[tokio::test]
async fn never_falls_back_to_plaintext() {
    let server = MockSieveServer::start(MockSieveConfig::default())
        .await
        .unwrap();
    // The server offers no STARTTLS: connecting with `starttls` fails.
    let err = SieveSession::connect(&config(&server, TlsMode::Starttls))
        .await
        .unwrap_err();
    assert_eq!(err, SieveError::StartTlsUnavailable);
    assert_eq!(server.logins(), 0);
    assert!(
        !server
            .commands()
            .iter()
            .any(|c| c.starts_with("AUTHENTICATE"))
    );
}

#[tokio::test]
async fn login_failures() {
    let server = MockSieveServer::start(MockSieveConfig {
        oauth_tokens: vec![(USER.into(), "tok".into())],
        ..MockSieveConfig::default()
    })
    .await
    .unwrap();
    let plain = config(&server, TlsMode::Plaintext);
    let err = SieveSession::login(&plain, &SieveCredentials::password(USER, "wrong"))
        .await
        .unwrap_err();
    assert!(matches!(err, SieveError::AuthFailed(_)));
    assert_eq!(err.localized().id, reasons::ERROR_AUTH);

    // OAuth2: XOAUTH2 by default, OAUTHBEARER when forced; a bad token gets
    // the error challenge and then NO.
    SieveSession::login(&plain, &SieveCredentials::oauth2(USER, "tok"))
        .await
        .unwrap();
    let bearer = SieveCredentials::OAuth2 {
        username: USER.into(),
        access_token: "tok".into(),
        mechanism: Some(SaslMechanism::Oauthbearer),
    };
    SieveSession::login(&plain, &bearer).await.unwrap();
    let err = SieveSession::login(&plain, &SieveCredentials::oauth2(USER, "old"))
        .await
        .unwrap_err();
    assert!(matches!(err, SieveError::AuthFailed(_)), "{err:?}");
    assert!(
        server
            .commands()
            .iter()
            .any(|c| c == "AUTHENTICATE OAUTHBEARER")
    );

    // No common mechanism.
    let oauth_only = MockSieveServer::start(MockSieveConfig {
        sasl: vec!["XOAUTH2".into()],
        ..MockSieveConfig::default()
    })
    .await
    .unwrap();
    let err = SieveSession::login(&config(&oauth_only, TlsMode::Plaintext), &password())
        .await
        .unwrap_err();
    assert_eq!(err, SieveError::AuthUnsupported("XOAUTH2".into()));
    let forced = SieveCredentials::OAuth2 {
        username: USER.into(),
        access_token: "tok".into(),
        mechanism: Some(SaslMechanism::Oauthbearer),
    };
    assert!(matches!(
        SieveSession::login(&config(&oauth_only, TlsMode::Plaintext), &forced).await,
        Err(SieveError::AuthUnsupported(_))
    ));
}

#[tokio::test]
async fn unavailable_servers() {
    // Nothing listens on the port.
    let port = {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        listener.local_addr().unwrap().port()
    };
    let refused = SieveConfig::new("127.0.0.1")
        .with_port(port)
        .with_tls(TlsMode::Plaintext)
        .with_timeout(Duration::from_secs(5));
    let err = SieveSession::connect(&refused).await.unwrap_err();
    assert!(err.is_unavailable(), "{err:?}");
    assert_eq!(err.localized().id, reasons::ERROR_CONNECT);

    // Another protocol on the port.
    let pop3 = MockPop3Server::start(MockPop3Config::default(), vec![])
        .await
        .unwrap();
    let other = refused.clone().with_port(pop3.port());
    let err = SieveSession::connect(&other).await.unwrap_err();
    assert!(matches!(err, SieveError::NotManageSieve(_)), "{err:?}");
    assert!(err.is_unavailable());
    assert_eq!(err.code(), ErrorCode::Unsupported);

    // A server that never answers.
    let silent = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let quiet = refused
        .with_port(silent.local_addr().unwrap().port())
        .with_timeout(Duration::from_millis(200));
    let err = SieveSession::connect(&quiet).await.unwrap_err();
    assert!(matches!(err, SieveError::Timeout(_)), "{err:?}");
}

/// A token provider that hands out a stale token until asked to refresh.
#[derive(Debug, Default)]
struct Tokens {
    calls: AtomicUsize,
    refreshed: Mutex<bool>,
}

impl AccessTokenProvider for Tokens {
    fn access_token<'a>(
        &'a self,
        provider: &'a str,
        _secret: &'a SecretRef,
        force_refresh: bool,
    ) -> BoxFuture<'a, Result<String, EngineError>> {
        Box::pin(async move {
            assert_eq!(provider, "google");
            self.calls.fetch_add(1, Ordering::SeqCst);
            let mut refreshed = self
                .refreshed
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            *refreshed |= force_refresh;
            Ok(if *refreshed { "tok" } else { "stale" }.to_owned())
        })
    }
}

#[tokio::test]
async fn sieve_auth_resolves_the_imap_login() {
    let server = MockSieveServer::start(MockSieveConfig {
        oauth_tokens: vec![(USER.into(), "tok".into())],
        ..MockSieveConfig::default()
    })
    .await
    .unwrap();
    let plain = config(&server, TlsMode::Plaintext);
    let store: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::new());
    let tokens = Arc::new(Tokens::default());
    let account = "work".into();

    let password_auth = AuthConfig::Password {
        username: USER.into(),
        secret: None,
        mechanism: None,
    };
    let auth = SieveAuth::from_config(&account, &password_auth, Arc::clone(&store), tokens.clone())
        .unwrap();
    // No password stored yet.
    let err = auth.login(&plain).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::AuthFailed);
    store
        .set(&password_auth.secret_ref(&account).unwrap(), PASSWORD)
        .unwrap();
    auth.login(&plain).await.unwrap();
    assert!(format!("{auth:?}").starts_with("StoredPassword"));
    let fixed = SieveAuth::Credentials(SieveCredentials::password(USER, "hunter2"));
    assert!(!format!("{fixed:?}").contains("hunter2"));

    // A stale OAuth2 token is refreshed once.
    let oauth = AuthConfig::OAuth2 {
        username: USER.into(),
        provider: "google".into(),
        secret: None,
        mechanism: None,
    };
    let auth =
        SieveAuth::from_config(&account, &oauth, Arc::clone(&store), tokens.clone()).unwrap();
    auth.login(&plain).await.unwrap();
    assert_eq!(tokens.calls.load(Ordering::SeqCst), 2);

    // Wrong credentials surface as an engine error with the Fluent text.
    let fixed = SieveAuth::Credentials(SieveCredentials::password(USER, "nope"));
    let err = fixed.login(&plain).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::AuthFailed);
    assert_eq!(err.localized.unwrap().id, reasons::ERROR_AUTH);

    assert!(SieveAuth::from_config(&account, &AuthConfig::None, store, tokens).is_err());
}

// ---------------------------------------------------------------------------
// The managed script

fn folders() -> FolderMap {
    FolderMap::new(Some('/'), &[(FolderRole::Archive, "Archive")])
}

fn options(folders: &FolderMap) -> CompileOptions<'_> {
    CompileOptions {
        account_id: "work".into(),
        account_aliases: vec![],
        // Replaced by the server's list during the upload.
        extensions: vec![],
        folders,
        own_addresses: vec![USER.into()],
        utc_offset_minutes: 0,
        today: Some("2026-09-30".into()),
        max_redirects: None,
        previous_active: None,
    }
}

fn rules() -> Vec<Rule> {
    vec![
        Rule {
            id: "newsletters".into(),
            name: "Newsletters".into(),
            enabled: true,
            triggers: vec![RuleTrigger::Arrival],
            scope: vec![],
            conditions: Condition::ListId { matches: None },
            actions: vec![RuleAction::Move {
                folder: FolderRef::Path {
                    path: "Reading".into(),
                    account_id: None,
                },
            }],
            source: None,
        },
        Rule {
            id: "boss".into(),
            name: "Boss".into(),
            enabled: true,
            triggers: vec![RuleTrigger::Arrival],
            scope: vec![],
            conditions: Condition::Body {
                matches: TextMatch {
                    op: TextOp::Contains,
                    value: "urgent".into(),
                    case_sensitive: false,
                },
            },
            actions: vec![RuleAction::Flag { flagged: true }],
            source: None,
        },
    ]
}

fn vacation() -> VacationSettings {
    VacationSettings {
        enabled: true,
        body: "Back on Monday.".into(),
        ..VacationSettings::default()
    }
}

#[tokio::test]
async fn upload_preserves_the_users_scripts() {
    let (server, mut session) = plain_server(MockSieveConfig::default()).await;
    let theirs = "require \"fileinto\";\nif header :contains \"subject\" \"[SPAM]\" { fileinto \"Junk\"; }\n";
    server.set_script("roundcube", theirs, true);
    server.set_script("old", "keep;", false);

    let folders = folders();
    let report = upload(
        &mut session,
        "nail",
        &rules(),
        &vacation(),
        options(&folders),
    )
    .await
    .unwrap();
    assert!(report.activated);
    assert_eq!(report.previous_active.as_deref(), Some("roundcube"));
    assert_eq!(report.other_scripts, ["old", "roundcube"]);
    assert!(report.extensions.contains(&"vacation".to_owned()));
    assert!(report.compiled.vacation.in_script());
    assert_eq!(report.compiled.server_rules(), ["newsletters", "boss"]);
    assert_eq!(report.compiled.included.as_deref(), Some("roundcube"));
    assert_eq!(
        report.script_hash,
        nail_sieve::script_hash(&report.compiled.script)
    );

    // What landed on the server: checked, uploaded, active; the user's
    // scripts untouched.
    assert_eq!(server.active().as_deref(), Some("nail"));
    let stored = server.script("nail").unwrap();
    assert_eq!(stored, to_crlf(&report.compiled.script));
    assert_eq!(server.checks(), std::slice::from_ref(&stored));
    assert_eq!(server.script("roundcube").as_deref(), Some(theirs));
    assert_eq!(server.script("old").as_deref(), Some("keep;"));
    let back = decompile(&stored).unwrap();
    assert!(back.managed);
    assert_eq!(back.included.as_deref(), Some("roundcube"));
    assert_eq!(back.previous_active.as_deref(), Some("roundcube"));
    assert_eq!(back.vacation.unwrap().body, "Back on Monday.");
    assert_eq!(back.rules.len(), 2);

    // Second upload: already active, the recorded previous script is kept.
    let report = upload(
        &mut session,
        "nail",
        &rules()[..1],
        &VacationSettings::default(),
        options(&folders),
    )
    .await
    .unwrap();
    assert!(!report.activated);
    assert_eq!(report.previous_active.as_deref(), Some("roundcube"));
    assert!(!report.compiled.vacation.in_script());
    assert_eq!(server.uploads().len(), 2);
    assert!(!server.script("nail").unwrap().contains("vacation"));

    // Switching Nail's script off gives the user's script back.
    let restored = remove(&mut session, "nail").await.unwrap();
    assert_eq!(restored.as_deref(), Some("roundcube"));
    assert_eq!(server.active().as_deref(), Some("roundcube"));
    assert_eq!(server.script("nail"), None);
    assert_eq!(remove(&mut session, "nail").await.unwrap(), None);
}

#[tokio::test]
async fn a_renamed_script_never_treats_nails_old_script_as_the_users() {
    let (server, mut session) = plain_server(MockSieveConfig::default()).await;
    server.set_script("roundcube", "keep;", true);
    let folders = folders();
    upload(
        &mut session,
        "nail",
        &rules(),
        &vacation(),
        options(&folders),
    )
    .await
    .unwrap();
    assert_eq!(server.active().as_deref(), Some("nail"));

    // The user picks another script name: the old Nail script is active.
    let report = upload(
        &mut session,
        "nail-work",
        &rules(),
        &vacation(),
        options(&folders),
    )
    .await
    .unwrap();
    assert!(report.activated);
    assert_eq!(report.replaced_managed.as_deref(), Some("nail"));
    assert_eq!(report.previous_active.as_deref(), Some("roundcube"));
    assert_eq!(report.compiled.included.as_deref(), Some("roundcube"));
    let stored = server.script("nail-work").unwrap();
    assert!(!stored.contains("include :personal \"nail\""), "{stored}");
    assert_eq!(server.active().as_deref(), Some("nail-work"));

    // A later upload keeps the user's script as the previous one.
    let report = upload(
        &mut session,
        "nail-work",
        &rules(),
        &vacation(),
        options(&folders),
    )
    .await
    .unwrap();
    assert_eq!(report.previous_active.as_deref(), Some("roundcube"));
    assert_eq!(report.replaced_managed, None);

    // Switching off brings the user's script back, not Nail's old one.
    assert_eq!(
        remove(&mut session, "nail-work").await.unwrap().as_deref(),
        Some("roundcube")
    );
    assert_eq!(server.active().as_deref(), Some("roundcube"));
}

#[tokio::test]
async fn a_recorded_previous_script_written_by_nail_is_followed() {
    let (server, mut session) = plain_server(MockSieveConfig::default()).await;
    server.set_script("mine", "keep;", false);
    // An older Nail script that recorded the user's script, and the
    // current one that (before the fix) recorded the older Nail script.
    server.set_script(
        "nail-old",
        &format!("{MANAGED_MARKER}\n{PREVIOUS_MARKER}\"mine\"\nkeep;\n"),
        false,
    );
    server.set_script(
        "nail",
        &format!("{MANAGED_MARKER}\n{PREVIOUS_MARKER}\"nail-old\"\nkeep;\n"),
        true,
    );
    assert_eq!(
        remove(&mut session, "nail").await.unwrap().as_deref(),
        Some("mine")
    );
    assert_eq!(server.active().as_deref(), Some("mine"));

    // A loop of Nail scripts ends with nothing active.
    server.set_script(
        "a",
        &format!("{MANAGED_MARKER}\n{PREVIOUS_MARKER}\"b\"\nkeep;\n"),
        false,
    );
    server.set_script(
        "b",
        &format!("{MANAGED_MARKER}\n{PREVIOUS_MARKER}\"a\"\nkeep;\n"),
        false,
    );
    server.set_script(
        "nail",
        &format!("{MANAGED_MARKER}\n{PREVIOUS_MARKER}\"a\"\nkeep;\n"),
        true,
    );
    assert_eq!(remove(&mut session, "nail").await.unwrap(), None);
    assert_eq!(server.active(), None);
}

#[tokio::test]
async fn upload_never_overwrites_a_foreign_script_with_our_name() {
    let (server, mut session) = plain_server(MockSieveConfig::default()).await;
    server.set_script("nail", "# written by hand\nkeep;\n", false);
    let folders = folders();
    let err = upload(
        &mut session,
        "nail",
        &rules(),
        &vacation(),
        options(&folders),
    )
    .await
    .unwrap_err();
    assert_eq!(err, SieveError::NotManaged("nail".into()));
    assert_eq!(err.code(), ErrorCode::Conflict);
    assert_eq!(err.localized().id, reasons::ERROR_NOT_MANAGED);
    assert_eq!(server.uploads().len(), 0);
    assert_eq!(server.active(), None);
    assert!(matches!(
        remove(&mut session, "nail").await,
        Err(SieveError::NotManaged(_))
    ));
    assert!(server.script("nail").is_some());
}

#[tokio::test]
async fn upload_is_capability_aware() {
    // An RFC 5228 server with fileinto and vacation only, without
    // VERSION (no CHECKSCRIPT) and without include.
    let (server, mut session) = plain_server(MockSieveConfig {
        extensions: vec!["fileinto".into(), "vacation".into()],
        version: false,
        ..MockSieveConfig::default()
    })
    .await;
    server.set_script("theirs", "keep;", true);
    let folders = folders();
    let report = upload(
        &mut session,
        "nail",
        &rules(),
        &vacation(),
        options(&folders),
    )
    .await
    .unwrap();
    // The move needs `variables`, the body test `body` (and the flag
    // `imap4flags`): both stay with Nail; the vacation reply is uploaded.
    let placements: Vec<RulePlacement> =
        report.compiled.rules.iter().map(|r| r.placement).collect();
    assert_eq!(placements, [RulePlacement::Client, RulePlacement::Client]);
    assert_eq!(report.compiled.rules[0].reasons[0].id, reasons::EXTENSION);
    assert!(report.compiled.vacation.in_script());
    assert_eq!(report.compiled.included, None);
    assert_eq!(server.checks().len(), 0);
    let stored = server.script("nail").unwrap();
    assert!(stored.contains("# nail:previous \"theirs\""));
    assert!(!stored.contains("include"));
    assert_eq!(server.active().as_deref(), Some("nail"));
    // Removing restores the user's script.
    assert_eq!(
        remove(&mut session, "nail").await.unwrap().as_deref(),
        Some("theirs")
    );
}

#[tokio::test]
async fn rejected_upload_changes_nothing() {
    let (server, mut session) = plain_server(MockSieveConfig::default()).await;
    server.reject_scripts_containing("vacation", "vacation replies are disabled by policy");
    let folders = folders();
    let err = upload(
        &mut session,
        "nail",
        &rules(),
        &vacation(),
        options(&folders),
    )
    .await
    .unwrap_err();
    assert_eq!(
        err,
        SieveError::ScriptRejected("vacation replies are disabled by policy".into())
    );
    let engine = EngineError::from(err);
    assert_eq!(engine.code, ErrorCode::Rejected);
    assert!(!engine.retryable);
    assert_eq!(server.uploads().len(), 0);
    assert_eq!(server.active(), None);
    // Nothing to reject without the vacation reply.
    upload(
        &mut session,
        "nail",
        &rules(),
        &VacationSettings::default(),
        options(&folders),
    )
    .await
    .unwrap();
    assert_eq!(server.active().as_deref(), Some("nail"));
}

/// ADR 0190: the connection log of a session holds the commands and
/// answers, never the password or a script.
#[tokio::test]
async fn the_connection_log_shows_commands_without_scripts() {
    let server = MockSieveServer::start(MockSieveConfig::default())
        .await
        .unwrap();
    let logs = nail_connlog::ConnectionLogs::new();
    let scope = nail_api::connlog::ConnectionLogScope::account(&"work".into());
    let config = config(&server, TlsMode::Plaintext).with_tap(logs.tap(&scope));
    let mut session = SieveSession::login(&config, &password()).await.unwrap();
    let script = "require \"fileinto\";\nif header :contains \"subject\" \"Salary\" {\n  fileinto \"Private\";\n}\n";
    session.put_script("nail", script).await.unwrap();
    session.get_script("nail").await.unwrap();
    session.logout().await.unwrap();
    let text = logs.text(&scope);
    assert!(
        text.contains("C: AUTHENTICATE \"PLAIN\" <redacted>"),
        "{text}"
    );
    assert!(text.contains("C: PUTSCRIPT \"nail\" {"), "{text}");
    assert!(text.contains("C: GETSCRIPT \"nail\""), "{text}");
    assert!(text.contains(" bytes}"), "{text}");
    for secret in [PASSWORD, "Salary", "Private", "fileinto \""] {
        assert!(!text.contains(secret), "{secret} in {text}");
    }
}
