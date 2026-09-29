//! `accounts.kdl`: reading, validation errors with positions, and the
//! format-preserving writer.

use nail_api::accounts::{
    AccountConfig, AuthConfig, BodyPolicy, ComposerMode, FolderMapping, FolderMatch, FolderPolicy,
    FolderSource, Identity, IdentityRule, ImapOptions, IncomingServer, MailStyleConfig,
    OAuthProviderConfig, OutgoingServer, Pop3Options, QuotePosting, SaslMechanism, SecretRef,
    ServerEndpoint, Signature, SmtpOptions, SyncPolicy, TlsMode, VirtualAccountConfig,
};
use nail_api::{AccountId, FolderRole};
use nail_config::{AccountsDocument, ConfigErrorKind, parse_accounts};
use pretty_assertions::assert_eq;

const FULL: &str = r#"// My accounts
undo-send-delay 5

oauth-provider "google" client-id="123.apps.googleusercontent.com" client-secret="keychain:nail/oauth/google"
oauth-provider "corp" client-id="nail" auth-url="https://login.corp.example/authorize" token-url="https://login.corp.example/token" {
    scopes "imap" "smtp"
}

// Work mail
account "work" name="Work" email="ada@work.example" {
    imap host="imap.work.example" port=993 tls="implicit" poll-interval=120 compress=#false max-connections=2 {
        auth "password" username="ada" secret="keychain:nail/work/password" mechanism="plain"
    }
    smtp host="smtp.work.example" tls="starttls" dsn=#true {
        auth "oauth2" provider="corp" username="ada@work.example"
    }
    identity "ada@work.example" name="Ada Lovelace" reply-to="team@work.example" composer="plain" posting="top" {
        bcc "archive@work.example"
        signature "Ada\nWork" html="<p>Ada</p>"
        mail-style css="p { margin: 0; }" code-theme="base16-ocean.dark"
    }
    identity "ada@work.example" id="work-alt" name="A. Lovelace"
    sync bodies="headers-only" {
        folder role="inbox" bodies="full" idle=#true
        folder path="Lists/rust" bodies="recent" days=30 idle=#true
        folder role="junk" enabled=#false
    }
}

account "home" name="Home" email="ada@home.example" enabled=#false save-sent-copy=#false {
    pop3 host="pop.home.example" tls="implicit" leave-on-server=#true delete-after-days=14 delete-when-deleted-locally=#true {
        auth "password"
    }
    smtp host="smtp.home.example" port=2525 tls="plaintext" {
        auth "none"
    }
}

virtual-account "everything" name="Everything" default-identity="ada@home.example" {
    members "work" "home"
    map "Receipts" {
        folder account="work" path="Receipts"
        folder account="home" path="Bills"
    }
    identity-rule "*@client.example" identity="work-alt"
}
"#;

fn secret(s: &str) -> Option<SecretRef> {
    Some(SecretRef::parse(s).unwrap())
}

fn endpoint(host: &str, port: u16, tls: TlsMode, auth: AuthConfig) -> ServerEndpoint {
    ServerEndpoint {
        host: host.into(),
        port,
        tls,
        auth,
    }
}

fn identity(account: &str, email: &str) -> Identity {
    Identity {
        id: email.into(),
        account_id: account.into(),
        email: email.into(),
        name: None,
        reply_to: None,
        bcc: vec![],
        signature: None,
        composer_mode: None,
        posting: None,
        mail_style: None,
        crypto: None,
    }
}

fn work() -> AccountConfig {
    AccountConfig {
        id: "work".into(),
        name: "Work".into(),
        email: "ada@work.example".into(),
        enabled: true,
        incoming: IncomingServer::Imap {
            server: endpoint(
                "imap.work.example",
                993,
                TlsMode::Implicit,
                AuthConfig::Password {
                    username: "ada".into(),
                    secret: secret("keychain:nail/work/password"),
                    mechanism: Some(SaslMechanism::Plain),
                },
            ),
            options: ImapOptions {
                poll_interval_secs: Some(120),
                compress: Some(false),
                max_connections: Some(2),
            },
        },
        outgoing: Some(OutgoingServer::Smtp {
            server: endpoint(
                "smtp.work.example",
                587,
                TlsMode::Starttls,
                AuthConfig::OAuth2 {
                    username: "ada@work.example".into(),
                    provider: "corp".into(),
                    secret: None,
                    mechanism: None,
                },
            ),
            options: SmtpOptions { dsn: Some(true) },
        }),
        identities: vec![
            Identity {
                name: Some("Ada Lovelace".into()),
                reply_to: Some("team@work.example".into()),
                bcc: vec!["archive@work.example".into()],
                signature: Some(Signature {
                    text: Some("Ada\nWork".into()),
                    html: Some("<p>Ada</p>".into()),
                    markdown: None,
                }),
                composer_mode: Some(ComposerMode::Plain),
                posting: Some(QuotePosting::Top),
                mail_style: Some(MailStyleConfig {
                    css: Some("p { margin: 0; }".into()),
                    code_theme: Some("base16-ocean.dark".into()),
                }),
                ..identity("work", "ada@work.example")
            },
            Identity {
                id: "work-alt".into(),
                name: Some("A. Lovelace".into()),
                ..identity("work", "ada@work.example")
            },
        ],
        sync: SyncPolicy {
            bodies: Some(BodyPolicy::HeadersOnly),
            folders: vec![
                FolderPolicy {
                    folder: FolderMatch::Role(FolderRole::Inbox),
                    bodies: Some(BodyPolicy::Full),
                    enabled: None,
                    idle: Some(true),
                },
                FolderPolicy {
                    folder: FolderMatch::Path("Lists/rust".into()),
                    bodies: Some(BodyPolicy::Recent { days: 30 }),
                    enabled: None,
                    idle: Some(true),
                },
                FolderPolicy {
                    folder: FolderMatch::Role(FolderRole::Junk),
                    bodies: None,
                    enabled: Some(false),
                    idle: None,
                },
            ],
        },
        save_sent_copy: None,
    }
}

fn home() -> AccountConfig {
    AccountConfig {
        id: "home".into(),
        name: "Home".into(),
        email: "ada@home.example".into(),
        enabled: false,
        incoming: IncomingServer::Pop3 {
            server: endpoint(
                "pop.home.example",
                995,
                TlsMode::Implicit,
                AuthConfig::Password {
                    username: "ada@home.example".into(),
                    secret: None,
                    mechanism: None,
                },
            ),
            options: Pop3Options {
                poll_interval_secs: None,
                leave_on_server: Some(true),
                delete_after_days: Some(14),
                delete_when_deleted_locally: Some(true),
            },
        },
        outgoing: Some(OutgoingServer::Smtp {
            server: endpoint(
                "smtp.home.example",
                2525,
                TlsMode::Plaintext,
                AuthConfig::None,
            ),
            options: SmtpOptions::default(),
        }),
        identities: vec![identity("home", "ada@home.example")],
        sync: SyncPolicy::default(),
        save_sent_copy: Some(false),
    }
}

fn everything() -> VirtualAccountConfig {
    VirtualAccountConfig {
        id: "everything".into(),
        name: "Everything".into(),
        members: vec!["work".into(), "home".into()],
        merge_roles: true,
        folder_mappings: vec![FolderMapping {
            name: "Receipts".into(),
            sources: vec![
                FolderSource {
                    account_id: "work".into(),
                    path: "Receipts".into(),
                },
                FolderSource {
                    account_id: "home".into(),
                    path: "Bills".into(),
                },
            ],
        }],
        default_identity: Some("ada@home.example".into()),
        identity_rules: vec![IdentityRule {
            pattern: "*@client.example".into(),
            identity: "work-alt".into(),
        }],
    }
}

#[test]
fn full_file_parses() {
    let config = parse_accounts(FULL, "accounts.kdl").unwrap();
    assert_eq!(config.undo_send_secs, Some(5));
    assert_eq!(
        config.oauth_providers,
        vec![
            OAuthProviderConfig {
                id: "google".into(),
                client_id: "123.apps.googleusercontent.com".into(),
                client_secret: secret("keychain:nail/oauth/google"),
                auth_url: None,
                token_url: None,
                scopes: vec![],
            },
            OAuthProviderConfig {
                id: "corp".into(),
                client_id: "nail".into(),
                client_secret: None,
                auth_url: Some("https://login.corp.example/authorize".into()),
                token_url: Some("https://login.corp.example/token".into()),
                scopes: vec!["imap".into(), "smtp".into()],
            },
        ]
    );
    assert_eq!(config.accounts, vec![work(), home()]);
    assert_eq!(config.virtual_accounts, vec![everything()]);
}

#[test]
fn minimal_account_gets_defaults() {
    let text = "account \"a\" email=\"a@example.org\" {\n    imap host=\"imap.example.org\"\n}\n";
    let config = parse_accounts(text, "accounts.kdl").unwrap();
    let account = &config.accounts[0];
    assert_eq!(account.name, "a");
    assert!(account.enabled);
    assert_eq!(account.outgoing, None);
    let server = account.incoming.server();
    assert_eq!((server.port, server.tls), (993, TlsMode::Implicit));
    assert_eq!(
        server.auth,
        AuthConfig::Password {
            username: "a@example.org".into(),
            secret: None,
            mechanism: None
        }
    );
    assert_eq!(
        server.auth.secret_ref(&account.id).unwrap().as_str(),
        "keychain:nail/a/password"
    );
    assert_eq!(account.identities, vec![identity("a", "a@example.org")]);
    assert_eq!(account.sync, SyncPolicy::default());
    assert_eq!(config.effective_undo_send_secs(), 10);

    let starttls = "account \"a\" email=\"a@x.org\" {\n    pop3 host=\"p\" tls=\"starttls\"\n    smtp host=\"s\" tls=\"starttls\"\n}\n";
    let config = parse_accounts(starttls, "accounts.kdl").unwrap();
    assert_eq!(config.accounts[0].incoming.server().port, 110);
    assert_eq!(
        config.accounts[0].outgoing.as_ref().unwrap().server().port,
        587
    );
}

#[test]
fn empty_file_is_valid() {
    let config = parse_accounts("// nothing yet\n", "accounts.kdl").unwrap();
    assert!(config.accounts.is_empty() && config.virtual_accounts.is_empty());
    assert!(
        parse_accounts("", "accounts.kdl")
            .unwrap()
            .accounts
            .is_empty()
    );
}

/// Parses `text` and returns `(line, column, message)` of the error.
fn error(text: &str) -> (u32, u32, String) {
    let err = parse_accounts(text, "accounts.kdl").expect_err("expected an error");
    assert_eq!(err.file, "accounts.kdl");
    (err.line, err.column, err.message)
}

fn account_with(body: &str) -> String {
    format!("account \"a\" email=\"a@example.org\" {{\n{body}\n}}\n")
}

#[test]
fn secrets_in_the_file_are_refused() {
    let (line, column, message) = error(&account_with(
        "    imap host=\"h\" {\n        auth \"password\" password=\"hunter2\"\n    }",
    ));
    assert_eq!((line, column), (3, 25));
    assert!(
        message.contains("never stored in accounts.kdl"),
        "{message}"
    );
    assert!(message.contains("keychain:"), "{message}");

    let (line, _, message) = error(&account_with(
        "    imap host=\"h\" {\n        auth \"password\" secret=\"hunter2\"\n    }",
    ));
    assert_eq!(line, 3);
    assert!(message.contains("OS keychain"), "{message}");

    let (_, _, message) =
        error("oauth-provider \"google\" client-id=\"x\" client-secret=\"abc\"\n");
    assert!(message.contains("client-secret"), "{message}");
}

#[test]
fn validation_errors_have_positions() {
    let cases: &[(&str, (u32, u32), &str)] = &[
        ("accounts \"a\"\n", (1, 1), "unknown node `accounts`"),
        (
            "account \"a\" email=\"a@example.org\"\n",
            (1, 1),
            "needs an incoming server",
        ),
        (
            "account \"a\" {\n    imap host=\"h\"\n}\n",
            (1, 1),
            "needs email=",
        ),
        (
            "account \"all\" email=\"a@example.org\" {\n    imap host=\"h\"\n}\n",
            (1, 9),
            "reserved",
        ),
        (
            "account \"Work\" email=\"a@example.org\" {\n    imap host=\"h\"\n}\n",
            (1, 9),
            "invalid name",
        ),
        (
            "account \"a\" email=\"nope\" {\n    imap host=\"h\"\n}\n",
            (1, 13),
            "email address",
        ),
        (
            &account_with("    imap host=\"h\" tls=\"ssl\""),
            (2, 19),
            "`tls` must be",
        ),
        (
            &account_with("    imap host=\"h\" port=70000"),
            (2, 19),
            "from 1 to 65535",
        ),
        (
            &account_with("    imap host=\"h\"\n    pop3 host=\"p\""),
            (3, 5),
            "exactly one incoming",
        ),
        (
            &account_with("    imap host=\"h\" poll-interval=5"),
            (2, 19),
            "from 30 to 86400",
        ),
        (
            &account_with("    imap host=\"h\" {\n        auth \"kerberos\"\n    }"),
            (3, 14),
            "unknown auth method",
        ),
        (
            &account_with(
                "    imap host=\"h\" {\n        auth \"password\" mechanism=\"xoauth2\"\n    }",
            ),
            (3, 25),
            "`mechanism` must be one of plain, login, scram-sha-256",
        ),
        (
            &account_with(
                "    imap host=\"h\" {\n        auth \"oauth2\" provider=\"yahoo\"\n    }",
            ),
            (1, 1),
            "unknown OAuth provider \"yahoo\"",
        ),
        (
            &account_with("    imap host=\"h\"\n    sync bodies=\"recent\""),
            (3, 10),
            "needs days=",
        ),
        (
            &account_with(
                "    imap host=\"h\"\n    sync {\n        folder role=\"other\" idle=#true\n    }",
            ),
            (4, 16),
            "`role` must be",
        ),
        (
            &account_with("    imap host=\"h\"\n    sync {\n        folder path=\"X\"\n    }"),
            (4, 9),
            "changes nothing",
        ),
        (
            &account_with("    pop3 host=\"h\" leave-on-server=#false delete-after-days=3"),
            (2, 42),
            "only applies with leave-on-server",
        ),
        (
            &account_with(
                "    imap host=\"h\"\n    identity \"x@y.org\"\n    identity \"x@y.org\"",
            ),
            (4, 5),
            "used twice",
        ),
        ("undo-send-delay 601\n", (1, 17), "from 0 to 600"),
        (
            "oauth-provider \"corp\" client-id=\"x\"\n",
            (1, 1),
            "needs auth-url",
        ),
        (
            "oauth-provider \"corp\" client-id=\"x\" auth-url=\"http://evil.example/a\" token-url=\"https://t\"\n",
            (1, 37),
            "https://",
        ),
    ];
    let mut mismatches = Vec::new();
    for (text, (line, column), needle) in cases {
        let (l, c, message) = error(text);
        assert!(message.contains(needle), "{text}\n→ {message}");
        if (l, c) != (*line, *column) {
            mismatches.push(format!(
                "{text:?}: expected {line}:{column}, got {l}:{c} ({message})"
            ));
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

#[test]
fn virtual_account_references_are_checked() {
    let accounts = "account \"a\" email=\"a@x.org\" {\n    imap host=\"h\"\n}\naccount \"b\" email=\"b@x.org\" {\n    imap host=\"h\"\n}\n";
    let check = |va: &str, needle: &str| {
        let text = format!("{accounts}{va}");
        let err = parse_accounts(&text, "accounts.kdl").expect_err(va);
        assert!(err.message.contains(needle), "{va}\n→ {}", err.message);
        err
    };
    let err = check(
        "virtual-account \"v\" {\n    members \"a\" \"c\"\n}\n",
        "unknown account \"c\"",
    );
    assert_eq!((err.line, err.column), (8, 17));
    check("virtual-account \"v\" {\n}\n", "needs `members");
    check(
        "virtual-account \"v\" {\n    members \"a\" \"a\"\n}\n",
        "listed twice",
    );
    check(
        "virtual-account \"a\" {\n    members \"b\"\n}\n",
        "already used",
    );
    check(
        "virtual-account \"v\" default-identity=\"b@x.org\" {\n    members \"a\"\n}\n",
        "not a member",
    );
    check(
        "virtual-account \"v\" default-identity=\"z@x.org\" {\n    members \"a\"\n}\n",
        "unknown identity",
    );
    check(
        "virtual-account \"v\" {\n    members \"a\"\n    map \"R\" {\n        folder account=\"b\" path=\"R\"\n    }\n}\n",
        "not a member",
    );
    check(
        "virtual-account \"v\" {\n    members \"a\"\n    identity-rule \"client.com\" identity=\"a@x.org\"\n}\n",
        "invalid pattern",
    );
    check(
        "virtual-account \"v\" {\n    members \"a\"\n}\nvirtual-account \"w\" {\n    members \"v\"\n}\n",
        "virtual accounts cannot be members",
    );
}

#[test]
fn syntax_errors_are_syntax_errors() {
    let err = parse_accounts("account \"a\" {\n", "accounts.kdl").unwrap_err();
    assert_eq!(err.kind, ConfigErrorKind::Syntax);
}

// ---------------------------------------------------------------------------
// Writer

fn doc(text: &str) -> AccountsDocument {
    AccountsDocument::parse(text, "accounts.kdl").unwrap()
}

#[test]
fn unchanged_config_leaves_the_text_alone() {
    let mut d = doc(FULL);
    let config = d.config().clone();
    d.set_config(config).unwrap();
    assert_eq!(d.text(), FULL);
}

#[test]
fn canonical_render_round_trips() {
    let mut d = AccountsDocument::empty("accounts.kdl");
    let config = parse_accounts(FULL, "accounts.kdl").unwrap();
    d.set_config(config.clone()).unwrap();
    assert_eq!(d.config(), &config);
    assert_eq!(parse_accounts(d.text(), "accounts.kdl").unwrap(), config);
    assert!(
        d.text().contains("account \"work\" name=\"Work\""),
        "{}",
        d.text()
    );
    assert!(!d.text().contains("password=\""), "{}", d.text());
}

#[test]
fn changing_one_value_changes_one_line() {
    let mut d = doc(FULL);
    let mut account = work();
    if let IncomingServer::Imap { server, .. } = &mut account.incoming {
        server.port = 1993;
    }
    d.upsert_account(account.clone()).unwrap();
    let edited = d.clone();
    let before: Vec<&str> = FULL.lines().collect();
    let after: Vec<&str> = d.text().lines().collect();
    assert_eq!(before.len(), after.len());
    let changed: Vec<(usize, &str)> = before
        .iter()
        .zip(&after)
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(i, (_, b))| (i, *b))
        .collect();
    assert_eq!(
        changed,
        vec![(
            10,
            "    imap host=\"imap.work.example\" port=1993 tls=\"implicit\" poll-interval=120 compress=#false max-connections=2 {"
        )]
    );
    assert_eq!(d.config().account(&"work".into()), Some(&account));

    // Undoing the edit restores the original text exactly.
    let mut d = edited;
    d.upsert_account(work()).unwrap();
    assert_eq!(d.text(), FULL);
}

#[test]
fn edits_keep_comments_and_unrelated_text() {
    let text = "// accounts of Ada\n\naccount \"a\" email=\"a@x.org\" {\n  // the server\n  imap host=\"h\" // no port: default\n  identity \"a@x.org\" name=\"Ada\" /* keep */\n}\n\n/-account \"old\" email=\"o@x.org\"\n\naccount \"b\" email=\"b@x.org\" {\n  imap host=\"hb\"\n}\n";
    let mut d = doc(text);

    // Changing account b leaves a (and its comments) untouched even though
    // a's canonical form would spell out port, tls and auth.
    let mut b = d.config().account(&"b".into()).unwrap().clone();
    b.name = "Bee".into();
    d.upsert_account(b).unwrap();
    assert!(d.text().starts_with(
        "// accounts of Ada\n\naccount \"a\" email=\"a@x.org\" {\n  // the server\n  imap host=\"h\" // no port: default\n  identity \"a@x.org\" name=\"Ada\" /* keep */\n}\n\n/-account \"old\" email=\"o@x.org\"\n"
    ), "{}", d.text());
    assert!(
        d.text()
            .contains("account \"b\" email=\"b@x.org\" name=\"Bee\" {\n  imap host=\"hb\""),
        "{}",
        d.text()
    );

    // Changing a's identity keeps the comment on the imap line.
    let mut a = d.config().account(&"a".into()).unwrap().clone();
    a.identities[0].name = Some("Ada L.".into());
    d.upsert_account(a).unwrap();
    // Values the reader fills in by default (port, tls, the password login)
    // are not spelled out just because the account was edited.
    assert!(
        d.text().contains("  imap host=\"h\" // no port: default\n"),
        "{}",
        d.text()
    );
    assert!(d.text().contains("/* keep */"), "{}", d.text());
    assert!(d.text().contains("name=\"Ada L.\""), "{}", d.text());
    assert!(d.text().contains("  // the server\n"), "{}", d.text());
}

#[test]
fn adding_and_removing_accounts() {
    let text =
        "// header\n\n// about a\naccount \"a\" email=\"a@x.org\" {\n    imap host=\"h\"\n}\n";
    let mut d = doc(text);
    let mut b = home();
    b.id = "b".into();
    b.identities = vec![identity("b", "ada@home.example")];
    d.upsert_account(b.clone()).unwrap();
    assert!(d.text().starts_with(text), "{}", d.text());
    assert!(
        d.text().contains("\n\naccount \"b\" name=\"Home\""),
        "{}",
        d.text()
    );
    assert_eq!(d.config().accounts.len(), 2);

    // Removing a takes its own comment along but keeps the file header.
    assert!(d.remove_account(&"a".into()).unwrap());
    assert!(!d.remove_account(&"a".into()).unwrap());
    assert!(d.text().starts_with("// header\n\n"), "{}", d.text());
    assert!(!d.text().contains("about a"), "{}", d.text());
    assert!(!d.text().contains("account \"a\""), "{}", d.text());
    assert_eq!(d.config().accounts, vec![b]);
}

#[test]
fn removing_an_account_cleans_up_virtual_accounts() {
    let mut d = doc(FULL);
    assert!(d.remove_account(&"home".into()).unwrap());
    let v = &d.config().virtual_accounts[0];
    assert_eq!(v.members, vec![AccountId::new("work")]);
    assert_eq!(v.folder_mappings[0].sources.len(), 1);
    assert_eq!(v.default_identity, None);
    assert_eq!(v.identity_rules.len(), 1);
    assert!(!d.text().contains("default-identity"), "{}", d.text());
    assert!(d.text().contains("    members \"work\"\n"), "{}", d.text());
    assert!(d.text().starts_with("// My accounts\nundo-send-delay 5\n"));

    // The last member gone: the virtual account goes too.
    assert!(d.remove_account(&"work".into()).unwrap());
    assert!(d.config().virtual_accounts.is_empty());
    assert!(d.config().accounts.is_empty());
    assert!(parse_accounts(d.text(), "accounts.kdl").is_ok());
}

#[test]
fn virtual_account_edits() {
    let mut d = doc(FULL);
    let mut v = everything();
    v.members.reverse();
    v.merge_roles = false;
    v.identity_rules.clear();
    d.upsert_virtual_account(v.clone()).unwrap();
    assert_eq!(d.config().virtual_accounts, vec![v]);
    assert!(
        d.text().contains("    members \"home\" \"work\"\n"),
        "{}",
        d.text()
    );
    assert!(d.text().contains("merge-roles=#false"), "{}", d.text());
    assert!(!d.text().contains("identity-rule"), "{}", d.text());
    assert!(d.remove_virtual_account(&"everything".into()).unwrap());
    assert!(!d.text().contains("virtual-account"), "{}", d.text());
    assert!(!d.remove_virtual_account(&"everything".into()).unwrap());
}

#[test]
fn switching_protocols_and_auth_methods() {
    let mut d = doc(FULL);
    let mut account = work();
    account.incoming = IncomingServer::Pop3 {
        server: endpoint(
            "pop.work.example",
            995,
            TlsMode::Implicit,
            AuthConfig::OAuth2 {
                username: "ada@work.example".into(),
                provider: "google".into(),
                secret: None,
                mechanism: Some(SaslMechanism::Xoauth2),
            },
        ),
        options: Pop3Options::default(),
    };
    account.outgoing = None;
    account.sync = SyncPolicy::default();
    d.upsert_account(account.clone()).unwrap();
    assert_eq!(d.config().account(&"work".into()), Some(&account));
    assert!(!d.text().contains("imap host"), "{}", d.text());
    assert!(!d.text().contains("smtp.work.example"), "{}", d.text());
    assert!(
        d.text().contains("    pop3 host=\"pop.work.example\""),
        "{}",
        d.text()
    );
    assert!(
        d.text().contains("auth \"oauth2\" provider=\"google\""),
        "{}",
        d.text()
    );
    assert!(!d.text().contains("sync bodies"), "{}", d.text());
    // The rest of the file is unchanged.
    let tail = FULL.split_at(FULL.find("account \"home\"").unwrap()).1;
    assert!(d.text().ends_with(tail), "{}", d.text());
}

#[test]
fn invalid_edits_are_refused_and_leave_the_document() {
    let mut d = doc(FULL);
    let before = d.clone();
    let mut bad = work();
    bad.id = "Not Valid".into();
    assert!(d.upsert_account(bad).is_err());
    assert_eq!(d, before);

    let mut v = everything();
    v.members.push("nobody".into());
    let err = d.upsert_virtual_account(v).unwrap_err();
    assert!(
        err.message.contains("unknown account \"nobody\""),
        "{}",
        err.message
    );
    assert_eq!(d, before);
}

#[test]
fn indentation_and_line_endings_follow_the_file() {
    let text = "account \"a\" email=\"a@x.org\" {\r\n  imap host=\"h\"\r\n}\r\n";
    let mut d = doc(text);
    let mut a = d.config().accounts[0].clone();
    a.outgoing = Some(OutgoingServer::Smtp {
        server: endpoint("s", 465, TlsMode::Implicit, AuthConfig::None),
        options: SmtpOptions::default(),
    });
    d.upsert_account(a.clone()).unwrap();
    assert_eq!(d.config().accounts[0], a);
    assert!(
        d.text().contains(
            "\r\n  smtp host=\"s\" port=465 tls=\"implicit\" {\r\n    auth \"none\"\r\n  }\r\n"
        ),
        "{:?}",
        d.text()
    );
    assert!(
        !d.text().replace("\r\n", "").contains('\n'),
        "{:?}",
        d.text()
    );
}

#[test]
fn strings_with_quotes_and_newlines_survive() {
    let mut d = AccountsDocument::empty("accounts.kdl");
    let mut account = home();
    account.name = "Home \"sweet\" \\ home".into();
    account.identities[0].signature = Some(Signature {
        text: Some("-- \nAda\ttab".into()),
        html: None,
        markdown: Some("**Ada**".into()),
    });
    let mut config = d.config().clone();
    config.accounts.push(account.clone());
    d.set_config(config).unwrap();
    assert_eq!(d.config().accounts[0], account);
}

#[test]
fn save_and_load() {
    let dir = tempfile::tempdir().unwrap();
    let empty = AccountsDocument::load(dir.path()).unwrap();
    assert!(empty.config().accounts.is_empty());
    let d = doc(FULL);
    d.save(dir.path()).unwrap();
    let text = std::fs::read_to_string(dir.path().join(nail_config::ACCOUNTS_FILE)).unwrap();
    assert_eq!(text, FULL);
    assert_eq!(AccountsDocument::load(dir.path()).unwrap(), d);
    assert_eq!(&nail_config::load_accounts(dir.path()).unwrap(), d.config());
    // Saving again replaces the file; no temporary file is left behind.
    d.save(dir.path()).unwrap();
    let names: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, vec![std::ffi::OsString::from("accounts.kdl")]);

    std::fs::write(dir.path().join("accounts.kdl"), "account {").unwrap();
    assert!(AccountsDocument::load(dir.path()).is_err());
}

#[test]
fn derived_ids_avoid_taken_names() {
    let d = doc(FULL);
    assert_eq!(d.derive_id("Work").as_str(), "work-2");
    assert_eq!(d.derive_id("everything").as_str(), "everything-2");
    assert_eq!(d.derive_id("new@mail.example").as_str(), "new-mail.example");
    assert!(d.is_taken("home") && !d.is_taken("all"));
}

#[test]
fn schema_lists_every_section() {
    let schema = nail_config::accounts_schema();
    let text = serde_json::to_string(&schema).unwrap();
    for needle in [
        "\"undo-send-delay\"",
        "\"oauth-provider\"",
        "\"account\"",
        "\"virtual-account\"",
        "\"imap\"",
        "\"pop3\"",
        "\"smtp\"",
        "\"identity\"",
        "\"sync\"",
        "\"leave-on-server\"",
        "\"delete-when-deleted-locally\"",
        "\"client-certificate\"",
        "\"identity-rule\"",
        "\"default-identity\"",
        "\"merge-roles\"",
        "\"save-sent-copy\"",
        "\"max-connections\"",
        "\"headers-only\"",
        "keychain:",
    ] {
        assert!(text.contains(needle), "accounts schema lacks {needle}");
    }
}

/// A sequence of edits of every kind; after each one the file must mean
/// exactly the desired configuration and still carry the user's comments
/// (the canonical fallback would drop them).
#[test]
fn many_edits_keep_meaning_and_comments() {
    let mut d = doc(FULL);
    type Edit = Box<dyn Fn(&mut nail_api::AccountsConfig)>;
    let edits: Vec<(&str, Edit)> = vec![
        ("undo delay", Box::new(|c| c.undo_send_secs = Some(30))),
        ("no undo delay", Box::new(|c| c.undo_send_secs = None)),
        (
            "rename work",
            Box::new(|c| c.accounts[0].name = "Office".into()),
        ),
        ("disable work", Box::new(|c| c.accounts[0].enabled = false)),
        ("enable work", Box::new(|c| c.accounts[0].enabled = true)),
        (
            "imap options",
            Box::new(|c| {
                if let IncomingServer::Imap { options, .. } = &mut c.accounts[0].incoming {
                    options.compress = None;
                    options.poll_interval_secs = Some(600);
                }
            }),
        ),
        (
            "auth username",
            Box::new(|c| {
                if let IncomingServer::Imap { server, .. } = &mut c.accounts[0].incoming {
                    server.auth = AuthConfig::Password {
                        username: "ada.l".into(),
                        secret: None,
                        mechanism: None,
                    };
                }
            }),
        ),
        (
            "add identity",
            Box::new(|c| {
                let mut extra = identity("work", "sales@work.example");
                extra.name = Some("Sales".into());
                c.accounts[0].identities.insert(1, extra);
            }),
        ),
        (
            "reorder identities",
            Box::new(|c| c.accounts[0].identities.swap(0, 1)),
        ),
        (
            "drop signature",
            Box::new(|c| {
                for i in &mut c.accounts[0].identities {
                    i.signature = None;
                    i.bcc.clear();
                }
            }),
        ),
        (
            "sync rules",
            Box::new(|c| {
                c.accounts[0].sync.folders.remove(1);
                c.accounts[0].sync.folders.push(FolderPolicy {
                    folder: FolderMatch::Role(FolderRole::Trash),
                    bodies: Some(BodyPolicy::HeadersOnly),
                    enabled: None,
                    idle: None,
                });
            }),
        ),
        (
            "no sync",
            Box::new(|c| c.accounts[0].sync = SyncPolicy::default()),
        ),
        ("smtp none", Box::new(|c| c.accounts[1].outgoing = None)),
        (
            "oauth scopes",
            Box::new(|c| c.oauth_providers[1].scopes = vec!["mail".into()]),
        ),
        (
            "drop google",
            Box::new(|c| {
                c.oauth_providers.remove(0);
            }),
        ),
        (
            "mapping",
            Box::new(|c| {
                c.virtual_accounts[0].folder_mappings.push(FolderMapping {
                    name: "Lists".into(),
                    sources: vec![FolderSource {
                        account_id: "work".into(),
                        path: "Lists/rust".into(),
                    }],
                })
            }),
        ),
        (
            "new account",
            Box::new(|c| {
                let mut third = home();
                third.id = "third".into();
                third.identities = vec![identity("third", "ada@home.example")];
                third.identities[0].id = "third-id".into();
                c.accounts.insert(1, third);
            }),
        ),
        (
            "member third",
            Box::new(|c| c.virtual_accounts[0].members.push("third".into())),
        ),
        (
            "move virtual",
            Box::new(|c| {
                let v = c.virtual_accounts.remove(0);
                c.virtual_accounts.push(v.clone());
                let mut w = v;
                w.id = "work-only".into();
                w.name = "Work only".into();
                w.members = vec!["work".into()];
                w.folder_mappings.clear();
                w.default_identity = None;
                w.identity_rules.clear();
                c.virtual_accounts.insert(0, w);
            }),
        ),
    ];
    for (label, edit) in edits {
        let mut config = d.config().clone();
        edit(&mut config);
        d.set_config(config.clone())
            .unwrap_or_else(|e| panic!("{label}: {e}\n{}", d.text()));
        assert_eq!(d.config(), &config, "{label}");
        assert_eq!(
            parse_accounts(d.text(), "accounts.kdl").unwrap(),
            config,
            "{label}"
        );
        assert!(
            d.text().starts_with("// My accounts\n"),
            "{label}:\n{}",
            d.text()
        );
        assert!(
            d.text().contains("// Work mail\n"),
            "{label}:\n{}",
            d.text()
        );
    }
}

#[test]
fn adding_to_a_comment_only_file_or_one_without_final_newline() {
    for text in [
        "// my accounts",
        "// my accounts\n",
        "/* block */",
        "account \"a\" email=\"a@x.org\" { imap host=\"h\"; }",
    ] {
        let mut d = doc(text);
        let mut b = home();
        b.id = "b".into();
        b.identities = vec![identity("b", "ada@home.example")];
        d.upsert_account(b.clone())
            .unwrap_or_else(|e| panic!("{text:?}: {e}"));
        assert_eq!(d.config().account(&"b".into()), Some(&b), "{text:?}");
        let first_line = text.lines().next().unwrap();
        assert!(d.text().starts_with(first_line), "{text:?}:\n{}", d.text());
    }
}

/// M2a (ADR 0029): an identity's `crypto` node is read, validated and
/// written back with only the values that differ from the defaults.
#[test]
fn identity_crypto_settings() {
    use nail_api::security::{AutocryptPreference, CryptoPreference, IdentityCrypto};

    let fpr = "0123 4567 89ab cdef 0123 4567 89AB CDEF 0123 4567";
    let text = account_with(&format!(
        "    imap host=\"h\"\n    identity \"a@example.org\" {{\n        crypto technology=\"openpgp\" sign=#true encrypt=#true openpgp-key=\"{fpr}\" prefer-encrypt=\"mutual\" protect-headers=#false\n    }}"
    ));
    let config = parse_accounts(&text, "accounts.kdl").unwrap();
    let crypto = config.accounts[0].identities[0].crypto.clone().unwrap();
    assert_eq!(
        crypto,
        IdentityCrypto {
            technology: CryptoPreference::OpenPgp,
            sign: true,
            encrypt: true,
            openpgp_key: Some("0123456789ABCDEF0123456789ABCDEF01234567".into()),
            smime_certificate: None,
            autocrypt: true,
            prefer_encrypt: AutocryptPreference::Mutual,
            protect_headers: false,
        }
    );

    // Identities without the node have none; an empty node means defaults.
    let plain = parse_accounts(&account_with("    imap host=\"h\""), "accounts.kdl").unwrap();
    assert_eq!(plain.accounts[0].identities[0].crypto, None);
    let empty = parse_accounts(
        &account_with(
            "    imap host=\"h\"\n    identity \"a@example.org\" {\n        crypto\n    }",
        ),
        "accounts.kdl",
    )
    .unwrap();
    assert_eq!(
        empty.accounts[0].identities[0].crypto,
        Some(IdentityCrypto::default())
    );

    // Canonical rendering writes only non-defaults and round-trips.
    let mut d = AccountsDocument::empty("accounts.kdl");
    d.set_config(config.clone()).unwrap();
    assert!(
        d.text().contains("crypto technology=\"openpgp\" sign=#true encrypt=#true openpgp-key=\"0123456789ABCDEF0123456789ABCDEF01234567\" prefer-encrypt=\"mutual\" protect-headers=#false"),
        "{}",
        d.text()
    );
    assert_eq!(parse_accounts(d.text(), "accounts.kdl").unwrap(), config);

    // Editing one setting in place keeps the rest of the line.
    let mut d = doc(&text);
    let mut account = d.config().accounts[0].clone();
    account.identities[0].crypto.as_mut().unwrap().sign = false;
    d.upsert_account(account.clone()).unwrap();
    assert!(!d.text().contains("sign=#true"), "{}", d.text());
    assert!(d.text().contains("encrypt=#true"), "{}", d.text());
    assert_eq!(d.config().accounts[0], account);

    // All defaults leave a bare node; `None` removes it.
    account.identities[0].crypto = Some(IdentityCrypto::default());
    d.upsert_account(account.clone()).unwrap();
    assert!(
        d.text().contains(
            "        crypto
"
        ),
        "{}",
        d.text()
    );
    assert_eq!(d.config().accounts[0], account);
    account.identities[0].crypto = None;
    d.upsert_account(account).unwrap();
    assert!(!d.text().contains("crypto"), "{}", d.text());

    // Validation.
    for (body, needle) in [
        ("crypto technology=\"gpg\"", "`technology` must be"),
        ("crypto prefer-encrypt=\"yes\"", "`prefer-encrypt` must be"),
        (
            "crypto openpgp-key=\"XYZ\"",
            "`openpgp-key` must be a fingerprint",
        ),
        (
            "crypto smime-cert=\"0123456789ABCDEF0123456789ABCDEF01234567\"",
            "64 hexadecimal",
        ),
        ("crypto bogus=#true", "bogus"),
        ("crypto\n        crypto", "`crypto` is set twice"),
        ("crypto \"x\"", ""),
    ] {
        let text = account_with(&format!(
            "    imap host=\"h\"\n    identity \"a@example.org\" {{\n        {body}\n    }}"
        ));
        let (line, _, message) = error(&text);
        assert!(line >= 4, "{body}: line {line}");
        assert!(message.contains(needle), "{body}: {message}");
    }
    let schema = serde_json::to_string(&nail_config::accounts_schema()).unwrap();
    assert!(schema.contains("\"prefer-encrypt\""), "{schema}");
}
