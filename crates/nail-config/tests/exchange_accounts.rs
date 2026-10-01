//! `accounts.kdl`: Exchange accounts (`exchange` node, Windows sign-in;
//! M3c, ADR 0072).

use nail_api::accounts::{AuthConfig, IncomingServer, MailProtocol, SspiPackage, TlsMode};
use nail_api::domain::AccountKind;
use nail_api::exchange::{ExchangeEngine, ExchangeOptions, GRAPH_HOST};
use nail_config::{AccountsDocument, parse_accounts};
use pretty_assertions::assert_eq;

const GRAPH: &str = r#"account "work" email="ada@contoso.example" {
    exchange {
        auth "oauth2" provider="microsoft"
    }
}
"#;

const EWS: &str = r#"account "corp" email="ada@corp.example" {
    exchange engine="ews" host="mail.corp.example" {
        auth "windows"
    }
}
"#;

#[test]
fn graph_account_gets_defaults() {
    let config = parse_accounts(GRAPH, "accounts.kdl").unwrap();
    let account = &config.accounts[0];
    assert_eq!(account.kind(), AccountKind::Exchange);
    assert!(account.outgoing.is_none() && account.can_send());
    assert_eq!(account.incoming.protocol(), MailProtocol::Graph);
    let IncomingServer::Exchange { server, options } = &account.incoming else {
        panic!("not an Exchange account: {:?}", account.incoming);
    };
    assert_eq!(
        (server.host.as_str(), server.port, server.tls),
        (GRAPH_HOST, 443, TlsMode::Implicit)
    );
    assert_eq!(options, &ExchangeOptions::default());
    assert!(matches!(
        &server.auth,
        AuthConfig::OAuth2 { provider, username, .. }
            if provider == "microsoft" && username == "ada@contoso.example"
    ));
}

#[test]
fn ews_account_with_windows_sign_in() {
    let config = parse_accounts(EWS, "accounts.kdl").unwrap();
    let account = &config.accounts[0];
    assert_eq!(account.incoming.protocol(), MailProtocol::Ews);
    let IncomingServer::Exchange { server, options } = &account.incoming else {
        panic!("not an Exchange account");
    };
    assert_eq!(server.host, "mail.corp.example");
    assert_eq!(options.engine, ExchangeEngine::Ews);
    assert!(options.streaming() && options.server_rules());
    assert_eq!(
        options.ews_url(server),
        "https://mail.corp.example/EWS/Exchange.asmx"
    );
    assert_eq!(
        server.auth,
        AuthConfig::Windows {
            username: None,
            secret: None,
            package: None
        }
    );

    // Explicit credentials, a package, the Autodiscover URL (its host is
    // the server's) and every option.
    let text = r#"account "corp" email="ada@corp.example" {
    exchange engine="ews" url="https://ews.corp.example/EWS/Exchange.asmx" streaming=#false server-rules=#false poll-interval=20 max-poll-interval=120 {
        auth "windows" username="CORP\\ada" package="ntlm"
    }
}
"#;
    let config = parse_accounts(text, "accounts.kdl").unwrap();
    let IncomingServer::Exchange { server, options } = &config.accounts[0].incoming else {
        panic!("not an Exchange account");
    };
    assert_eq!(server.host, "ews.corp.example");
    assert!(!options.streaming() && !options.server_rules());
    assert_eq!(options.poll_range(), (20, 120));
    assert_eq!(
        server.auth,
        AuthConfig::Windows {
            username: Some("CORP\\ada".into()),
            secret: None,
            package: Some(SspiPackage::Ntlm)
        }
    );
    assert_eq!(
        server.auth.secret_ref(&"corp".into()).unwrap().as_str(),
        "keychain:nail/corp/password"
    );
}

fn error(text: &str) -> String {
    parse_accounts(text, "accounts.kdl")
        .expect_err("expected an error")
        .message
}

#[test]
fn exchange_rules_are_checked() {
    let graph_password = r#"account "w" email="ada@contoso.example" {
    exchange
}
"#;
    assert!(
        error(graph_password).contains("oauth2"),
        "{}",
        error(graph_password)
    );
    let ews_without_host = r#"account "w" email="ada@corp.example" {
    exchange engine="ews" {
        auth "windows"
    }
}
"#;
    assert!(
        error(ews_without_host).contains("host"),
        "{}",
        error(ews_without_host)
    );
    let smtp = r#"account "w" email="ada@contoso.example" {
    exchange {
        auth "oauth2" provider="microsoft"
    }
    smtp host="smtp.office365.com"
}
"#;
    assert!(error(smtp).contains("exchange"), "{}", error(smtp));
    let plaintext = r#"account "w" email="ada@corp.example" {
    exchange engine="ews" host="mail.corp.example" tls="plaintext" {
        auth "windows"
    }
}
"#;
    assert!(
        error(plaintext).contains("localhost"),
        "{}",
        error(plaintext)
    );
    let bad_engine = r#"account "w" email="ada@corp.example" {
    exchange engine="eas" {
        auth "oauth2" provider="microsoft"
    }
}
"#;
    assert!(error(bad_engine).contains("ews"), "{}", error(bad_engine));
    let graph_streaming = r#"account "w" email="ada@contoso.example" {
    exchange streaming=#true {
        auth "oauth2" provider="microsoft"
    }
}
"#;
    assert!(
        error(graph_streaming).contains("ews"),
        "{}",
        error(graph_streaming)
    );
    let bad_url = r#"account "w" email="ada@corp.example" {
    exchange engine="ews" host="mail.corp.example" url="ftp://mail.corp.example/ews" {
        auth "windows"
    }
}
"#;
    assert!(error(bad_url).contains("ftp://"), "{}", error(bad_url));
    let windows_on_imap = r#"account "w" email="ada@corp.example" {
    imap host="imap.corp.example" {
        auth "windows"
    }
}
"#;
    assert!(
        error(windows_on_imap).contains("Windows"),
        "{}",
        error(windows_on_imap)
    );
    let bad_package = r#"account "w" email="ada@corp.example" {
    exchange engine="ews" host="mail.corp.example" {
        auth "windows" package="digest"
    }
}
"#;
    assert!(
        error(bad_package).contains("negotiate"),
        "{}",
        error(bad_package)
    );
    let secret_without_user = r#"account "w" email="ada@corp.example" {
    exchange engine="ews" host="mail.corp.example" {
        auth "windows" secret="keychain:nail/w/password"
    }
}
"#;
    assert!(
        error(secret_without_user).contains("username"),
        "{}",
        error(secret_without_user)
    );
    let sieve = r#"account "w" email="ada@contoso.example" {
    exchange {
        auth "oauth2" provider="microsoft"
    }
    sieve
}
"#;
    assert!(error(sieve).contains("IMAP"), "{}", error(sieve));
}

#[test]
fn exchange_accounts_are_written_and_round_trip() {
    for text in [GRAPH, EWS] {
        let config = parse_accounts(text, "accounts.kdl").unwrap();
        let mut d = AccountsDocument::empty("accounts.kdl");
        d.set_config(config.clone()).unwrap();
        assert!(d.text().contains("exchange engine="), "{}", d.text());
        assert_eq!(parse_accounts(d.text(), "accounts.kdl").unwrap(), config);
    }

    // Editing keeps the hand-written file minimal.
    let config = parse_accounts(GRAPH, "accounts.kdl").unwrap();
    let mut d = AccountsDocument::parse(GRAPH, "accounts.kdl").unwrap();
    let mut account = config.accounts[0].clone();
    if let IncomingServer::Exchange { options, .. } = &mut account.incoming {
        options.server_rules = Some(false);
    }
    d.upsert_account(account.clone()).unwrap();
    assert!(d.text().contains("server-rules=#false"), "{}", d.text());
    assert!(!d.text().contains(GRAPH_HOST), "{}", d.text());
    assert!(!d.text().contains("engine="), "{}", d.text());
    assert_eq!(d.config().accounts[0], account);

    // Windows sign-in with explicit credentials is written back.
    let mut d = AccountsDocument::parse(EWS, "accounts.kdl").unwrap();
    let mut account = parse_accounts(EWS, "accounts.kdl").unwrap().accounts[0].clone();
    if let IncomingServer::Exchange { server, .. } = &mut account.incoming {
        server.auth = AuthConfig::Windows {
            username: Some("ada@corp.example".into()),
            secret: None,
            package: Some(SspiPackage::Kerberos),
        };
    }
    d.upsert_account(account.clone()).unwrap();
    assert!(d.text().contains("package=\"kerberos\""), "{}", d.text());
    assert_eq!(d.config().accounts[0], account);
}
