//! `accounts.kdl`: Gmail API accounts (`gmail` node, M3b, ADR 0065).
#![cfg(test)]

#[expect(dead_code, reason = "each test crate uses part of the shared helpers")]
mod common;

use nail_api::accounts::{AuthConfig, IncomingServer, MailProtocol, TlsMode, default_port};
use nail_api::domain::AccountKind;
use nail_api::gmail::{GMAIL_API_HOST, GmailOptions};
use nail_config::{AccountsDocument, parse_accounts};
use pretty_assertions::assert_eq;

const GMAIL: &str = r#"account "gm" email="ada@gmail.example" {
    gmail {
        auth "oauth2" provider="google"
    }
}
"#;

#[test]
fn gmail_account_gets_api_defaults() {
    let config = parse_accounts(GMAIL, "accounts.kdl").unwrap();
    let account = &config.accounts[0];
    assert_eq!(account.kind(), AccountKind::Gmail);
    assert!(account.outgoing.is_none());
    assert!(account.can_send());
    assert!(account.incoming.sends_itself());
    assert_eq!(account.incoming.protocol(), MailProtocol::Gmail);
    let IncomingServer::Gmail { server, options } = &account.incoming else {
        panic!("not a Gmail account: {:?}", account.incoming);
    };
    assert_eq!(server.host, GMAIL_API_HOST);
    assert_eq!(server.port, 443);
    assert_eq!(server.tls, TlsMode::Implicit);
    assert_eq!(options, &GmailOptions::default());
    assert!(matches!(
        &server.auth,
        AuthConfig::OAuth2 { provider, username, .. }
            if provider == "google" && username == "ada@gmail.example"
    ));
}

#[test]
fn gmail_test_server_options_are_read() {
    let text = r#"account "gm" email="ada@gmail.example" {
    gmail host="127.0.0.1" port=8099 tls="plaintext" poll-interval=10 max-poll-interval=60 {
        auth "oauth2" provider="mock"
    }
}
oauth-provider "mock" client-id="nail" auth-url="http://127.0.0.1:1/auth" token-url="http://127.0.0.1:1/token"
"#;
    let config = parse_accounts(text, "accounts.kdl").unwrap();
    let IncomingServer::Gmail { server, options } = &config.accounts[0].incoming else {
        panic!("not a Gmail account");
    };
    assert_eq!(
        (server.host.as_str(), server.port, server.tls),
        ("127.0.0.1", 8099, TlsMode::Plaintext)
    );
    assert_eq!(options.poll_range(), (10, 60));
    assert_eq!(default_port(MailProtocol::Gmail, TlsMode::Plaintext), 80);
}

fn error(text: &str) -> String {
    parse_accounts(text, "accounts.kdl")
        .expect_err("expected an error")
        .message
}

#[test]
fn gmail_rules_are_checked() {
    let password = r#"account "gm" email="ada@gmail.example" {
    gmail
}
"#;
    assert!(error(password).contains("oauth2"), "{}", error(password));
    let smtp = r#"account "gm" email="ada@gmail.example" {
    gmail {
        auth "oauth2" provider="google"
    }
    smtp host="smtp.gmail.com"
}
"#;
    assert!(error(smtp).contains("smtp"), "{}", error(smtp));
    let plaintext = r#"account "gm" email="ada@gmail.example" {
    gmail host="gmail.example" tls="plaintext" {
        auth "oauth2" provider="google"
    }
}
"#;
    assert!(
        error(plaintext).contains("localhost"),
        "{}",
        error(plaintext)
    );
    let fast = r#"account "gm" email="ada@gmail.example" {
    gmail poll-interval=1 {
        auth "oauth2" provider="google"
    }
}
"#;
    assert!(error(fast).contains("poll-interval"), "{}", error(fast));
    let both = r#"account "gm" email="ada@gmail.example" {
    imap host="imap.gmail.com"
    gmail {
        auth "oauth2" provider="google"
    }
}
"#;
    parse_accounts(both, "accounts.kdl").expect_err("two incoming servers");
}

#[test]
fn gmail_accounts_are_written_and_round_trip() {
    let config = parse_accounts(GMAIL, "accounts.kdl").unwrap();
    common::assert_renders(&config, "gmail host=");

    // Editing keeps the hand-written file minimal (default host / port /
    // tls are not added).
    let mut d = AccountsDocument::parse(GMAIL, "accounts.kdl").unwrap();
    let mut account = config.accounts[0].clone();
    account.name = "Personal".into();
    if let IncomingServer::Gmail { options, .. } = &mut account.incoming {
        options.poll_interval_secs = Some(60);
    }
    d.upsert_account(account.clone()).unwrap();
    assert!(d.text().contains("name=\"Personal\""), "{}", d.text());
    assert!(d.text().contains("poll-interval=60"), "{}", d.text());
    assert!(!d.text().contains(GMAIL_API_HOST), "{}", d.text());
    assert_eq!(d.config().accounts[0], account);
}
