//! Shared helpers of the IMAP client integration tests.

use std::time::Duration;

use nail_api::FolderRole;
use nail_proto_imap::{Credentials, ImapClient, ImapConfig, Security, TlsTrust};
use nail_testkit::{MailStore, MockCapabilities, MockImapConfig, MockImapServer, NO_FLAGS};

pub(crate) const TIMEOUT: Duration = Duration::from_secs(10);
pub(crate) const TOKEN: &str = "ya29.mock-access-token";

/// A small message with a subject.
pub(crate) fn message(subject: &str) -> Vec<u8> {
    format!(
        "From: Ada <ada@example.org>\r\nTo: sam@nail.example\r\nSubject: {subject}\r\n\
         Date: Mon, 28 Sep 2026 08:15:00 +0200\r\nMessage-ID: <{subject}@example.org>\r\n\r\n\
         Hello {subject}.\r\n"
    )
    .into_bytes()
}

/// A store with `INBOX` (three messages: one seen, one flagged, one plain),
/// `Archive` (empty) and `Trash` (empty).
pub(crate) fn store() -> MailStore {
    let store = MailStore::new();
    store.add_mailbox("Archive", FolderRole::Archive);
    store.add_mailbox("Trash", FolderRole::Trash);
    store.append("INBOX", &message("one"), &["\\Seen"]);
    store.append("INBOX", &message("two"), &["\\Flagged"]);
    store.append("INBOX", &message("three"), NO_FLAGS);
    store
}

pub(crate) async fn server_with(capabilities: MockCapabilities) -> MockImapServer {
    MockImapServer::start(
        MockImapConfig {
            capabilities,
            oauth_token: Some(TOKEN.to_owned()),
            ..MockImapConfig::default()
        },
        store(),
    )
    .await
    .unwrap()
}

/// A modern server (every extension) with the [`store`] contents.
pub(crate) async fn modern_server() -> MockImapServer {
    server_with(MockCapabilities::default()).await
}

/// A bare IMAP4rev1 server with the [`store`] contents.
pub(crate) async fn bare_server() -> MockImapServer {
    server_with(MockCapabilities::imap4rev1()).await
}

pub(crate) fn config_for(server: &MockImapServer) -> ImapConfig {
    let mut config = ImapConfig::new(server.host())
        .with_port(server.port())
        .with_security(Security::InsecurePlaintext)
        .with_timeout(TIMEOUT);
    if let Some(ca) = server.ca_certificate_der() {
        config = config
            .with_security(Security::ImplicitTls)
            .with_tls_trust(TlsTrust::CustomRoots(vec![ca.to_vec()]));
    }
    config
}

pub(crate) fn credentials(server: &MockImapServer) -> Credentials {
    Credentials::new(&server.config().username, &server.config().password)
}

pub(crate) async fn login(server: &MockImapServer) -> ImapClient {
    ImapClient::connect(&config_for(server), &credentials(server))
        .await
        .unwrap()
}

/// Flags of a message on the server, sorted, for comparisons.
pub(crate) fn server_flags(server: &MockImapServer, mailbox: &str, uid: u32) -> Vec<String> {
    let mut flags = server.store().flags(mailbox, uid).unwrap_or_default();
    flags.sort();
    flags
}

/// UIDs of a mailbox on the server.
pub(crate) fn server_uids(server: &MockImapServer, mailbox: &str) -> Vec<u32> {
    server
        .store()
        .mailbox(mailbox)
        .map(|m| m.messages.iter().map(|m| m.uid).collect())
        .unwrap_or_default()
}
