//! Demo data and in-process mock servers, for development and Playwright:
//!
//! * `nail serve --seed demo`: the `nail-testkit` corpus as a local account;
//! * `--seed demo-imap`: the corpus on a mock IMAP server, synced like a
//!   real account;
//! * `--seed demo-multi` ([`start_demo_multi`]): three configured accounts
//!   (IMAP "work" with IDLE, POP3 "home", and — switched off until enabled
//!   in the settings — Gmail API "gmail" on the mock Gmail server with a
//!   mock OAuth2 authorization server; M3b), one SMTP
//!   server for the first two, a third IMAP server plus autoconfig / DNS
//!   mocks for setting up a new account by discovery, and a mock CalDAV /
//!   CardDAV server with a second principal (`bob`, free/busy; M3b). Secrets live in memory, never in the OS
//!   keychain. [`DemoMulti::control`] offers the test controls the
//!   Playwright suite uses (deliver mail, change flags, go offline, read
//!   what SMTP received).
//!
//! Every seeded run keeps its secrets in memory, trusts only the S/MIME
//! test root of the corpus and looks up no keys online
//! ([`demo_options`]); `demo-multi` also gives both accounts an OpenPGP
//! key and an S/MIME certificate ([`DemoMulti::seed_security`]).
//!
//! No network access beyond `127.0.0.1` is involved in any of them: the
//! remote-content proxy of a seeded run resolves no names
//! ([`offline_fetcher`]); demo-multi's resolves two made-up hosts only —
//! [`MULTI_IMAGES_HOST`] to its mock HTTP server and
//! [`MULTI_PRIVATE_HOST`] to a private address the proxy must refuse.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use nail_api::accounts::SecretKind;
use nail_api::engine::BoxFuture;
use nail_api::engine::{MemorySecretStore, SecretStore};
use nail_api::security::{KeyAlgorithm, SecretProtection};
use nail_api::{Account, AccountId, AccountKind, Folder, FolderRole, SecretRef};
use nail_discovery::{Discoverer, HickoryDns, ReqwestFetch, TcpProbe};
use nail_exchange::testing::{MockAutodiscoverServer, MockHost, MockV2};
use nail_pgp::discovery::{HttpClient, HttpResponse};
use nail_proto_ews::testing::{MockAuth, MockEwsServer};
use nail_proto_gmail::testing::MockGmailServer;
use nail_proto_graph::testing::MockGraphServer;
use nail_proto_imap::{Credentials, ImapConfig, Security};
use nail_store::{BlobStore, Store};
use nail_testkit::{
    CryptoFixtures, MailStore, MockImapConfig, MockImapServer, crypto_fixture, demo_corpus,
};
use nail_testkit_net::{
    DnsZone, MockDavConfig, MockDavServer, MockDavUser, MockDnsServer, MockHttpServer,
    MockOAuthConfig, MockOAuthServer, MockPop3Config, MockPop3Server, MockResponse,
    MockSieveConfig, MockSieveServer, MockSmtpConfig, MockSmtpServer, Pop3Message,
};
use serde_json::{Value, json};

use crate::error::Result;
use crate::ids;
use crate::ingest::{IncomingMessage, ingest};
use crate::sync::ImapAccount;

/// Id of the local demo account created by [`seed_demo`].
pub const DEMO_ACCOUNT_ID: &str = "demo";
/// Id of the IMAP demo account served by [`start_demo_imap`].
pub const DEMO_IMAP_ACCOUNT_ID: &str = "demo-imap";

/// First line of the `accounts.kdl` that `--seed demo-multi` writes; marks
/// a config directory as a demo one that may be overwritten.
pub const MULTI_CONFIG_MARKER: &str =
    "// Demo accounts of `nail serve --seed demo-multi` (mock servers on 127.0.0.1).";

/// The address the demo corpus is written to.
const DEMO_EMAIL: &str = "sam@nail.example";

/// A made-up image host of demo-multi: the proxy reaches it on the mock
/// HTTP server (loopback, allowed only for that port in seeded runs).
pub const MULTI_IMAGES_HOST: &str = "images.nail.example";
/// A made-up host of demo-multi that resolves to a private address (the
/// proxy refuses it).
pub const MULTI_PRIVATE_HOST: &str = "private.nail.example";

/// The folder of the "work" account synced headers only (M2c): its bodies
/// are fetched when a message is opened.
pub const MULTI_HEADERS_ONLY_FOLDER: &str = "Newsletters";

/// The message waiting in [`MULTI_HEADERS_ONLY_FOLDER`]; its body holds a
/// word found only by the server search (`platypus`).
pub const MULTI_NEWSLETTER: &str = "From: Nature Weekly <news@nature.example>
To: ada@work.example
Subject: This week in nature
Date: Tue, 29 Sep 2026 07:00:00 +0000
Message-ID: <weekly-39@nature.example>

This week: the platypus lays eggs and glows under ultraviolet light.
";

/// A 1×1 PNG (the demo image).
const PIXEL_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

/// The remote-content fetcher of seeded runs: every name is unknown, so
/// nothing is fetched (allowed content fails with `502`).
pub fn offline_fetcher() -> Option<nail_proxy::Fetcher> {
    let tls = nail_auth::tls_config().ok()?;
    Some(
        nail_proxy::Fetcher::new(Arc::new(tls))
            .with_resolver(nail_proxy::testing::ScriptedResolver::new()),
    )
}

/// Password of the corpus' PKCS#12 files.
pub const DEMO_PKCS12_PASSWORD: &str = "nail-test";

/// The `authserv-id` of the demo-multi "work" account's receiving server:
/// `Authentication-Results` headers from it are trusted.
pub const MULTI_AUTHSERV_ID: &str = "mx.nail.example";

/// Online key lookups of seeded runs: nothing leaves the machine, every
/// address is simply not found.
#[derive(Debug, Clone, Copy, Default)]
pub struct OfflineKeyLookup;

impl HttpClient for OfflineKeyLookup {
    fn get<'a>(
        &'a self,
        _url: &'a str,
    ) -> BoxFuture<'a, std::result::Result<HttpResponse, nail_api::engine::EngineError>> {
        Box::pin(async {
            Ok(HttpResponse {
                status: 404,
                body: Vec::new(),
            })
        })
    }
}

/// The S/MIME test root of the corpus as the only trust anchor.
pub fn test_trust_anchors() -> Arc<dyn nail_smime::TrustAnchors> {
    let roots = crypto_fixture(CryptoFixtures::Smime, "root-ca.crt")
        .and_then(|der| nail_smime::Cert::parse_many(&der).ok())
        .unwrap_or_default();
    Arc::new(nail_smime::FixedTrustAnchors::new(roots))
}

/// Options for a seeded (demo or test) run: secrets in memory (never the
/// OS keychain), the corpus' S/MIME test root as the only trust anchor,
/// no online key lookups.
pub fn demo_options(mut options: crate::ServiceOptions) -> crate::ServiceOptions {
    if options.secrets.is_none() {
        options.secrets = Some(Arc::new(MemorySecretStore::new()) as Arc<dyn SecretStore>);
    }
    options.trust_anchors = Some(test_trust_anchors());
    options.key_lookup = Some(Arc::new(OfflineKeyLookup));
    if options.proxy_fetcher.is_none() {
        options.proxy_fetcher = offline_fetcher();
    }
    // Subscriptions to the mock HTTP server (plain http on loopback).
    options.loopback_feeds = true;
    options
}

/// What [`seed_demo`] wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DemoSeed {
    /// The demo account.
    pub account: Account,
    /// Its folders, in corpus order (INBOX first).
    pub folders: Vec<Folder>,
    /// Number of messages written.
    pub messages: usize,
}

/// Writes the demo corpus into `store` and `blobs` as a local account
/// ([`DEMO_ACCOUNT_ID`]). Idempotent: ids are derived from the corpus, so
/// seeding again updates the same rows. Blocking.
pub fn seed_demo(store: &Store, blobs: &BlobStore) -> Result<DemoSeed> {
    let account = Account {
        id: AccountId::new(DEMO_ACCOUNT_ID),
        name: "Demo".to_owned(),
        email: DEMO_EMAIL.to_owned(),
        kind: AccountKind::Local,
    };
    store.upsert_account(&account)?;

    let mut folders = Vec::new();
    let mut messages = 0;
    for corpus_folder in demo_corpus() {
        let folder = Folder {
            id: ids::folder_id(&account.id, &corpus_folder.name),
            account_id: account.id.clone(),
            parent_id: None,
            name: corpus_folder.name.clone(),
            path: corpus_folder.name.clone(),
            role: corpus_folder.role,
            unread: 0,
            total: 0,
        };
        store.upsert_folder(&folder)?;
        let incoming: Vec<IncomingMessage> = corpus_folder
            .messages
            .iter()
            .zip(1u32..)
            .map(|(message, uid)| IncomingMessage {
                uid: Some(uid),
                flags: message.api_flags(),
                raw: message.raw.clone(),
                ..IncomingMessage::default()
            })
            .collect();
        messages += ingest(store, blobs, None, &folder, 1, &incoming)?
            .stored
            .len();
        folders.push(store.get_folder(&folder.id)?);
    }
    tracing::info!(folders = folders.len(), messages, "demo data seeded");
    Ok(DemoSeed {
        account,
        folders,
        messages,
    })
}

/// A running mock IMAP server with the demo corpus, plus the account and
/// settings to sync it. The server stops when this is dropped.
#[derive(Debug)]
pub struct DemoImap {
    /// The server (plaintext, on `127.0.0.1`).
    pub server: MockImapServer,
    /// The account to register ([`DEMO_IMAP_ACCOUNT_ID`]).
    pub account: Account,
    /// Settings pointing at the server.
    pub settings: ImapAccount,
}

/// Starts an in-process mock IMAP server serving the demo corpus.
pub async fn start_demo_imap() -> Result<DemoImap> {
    let server = MockImapServer::start_demo()
        .await
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    let settings = ImapAccount {
        config: ImapConfig::new(server.host())
            .with_port(server.port())
            .with_security(Security::InsecurePlaintext),
        credentials: Credentials::new(&server.config().username, &server.config().password),
    };
    let account = Account {
        id: AccountId::new(DEMO_IMAP_ACCOUNT_ID),
        name: "Demo (IMAP)".to_owned(),
        email: server.config().username.clone(),
        kind: AccountKind::Imap,
    };
    Ok(DemoImap {
        server,
        account,
        settings,
    })
}

// ---------------------------------------------------------------------------
// demo-multi

/// Id of the IMAP account of [`start_demo_multi`].
pub const MULTI_WORK_ID: &str = "work";
/// Id of the POP3 account of [`start_demo_multi`].
pub const MULTI_HOME_ID: &str = "home";
/// Address of the IMAP account.
pub const MULTI_WORK_EMAIL: &str = "ada@work.example";
/// Address of the POP3 account.
pub const MULTI_HOME_EMAIL: &str = "ada@home.example";
/// An address whose settings discovery finds (autoconfig on the mock HTTP
/// server), with its own mock IMAP server.
pub const MULTI_NEW_EMAIL: &str = "bob@new.example";
/// Password of the IMAP account.
pub const MULTI_WORK_PASSWORD: &str = "work-secret";
/// Password of the POP3 account.
pub const MULTI_HOME_PASSWORD: &str = "home-secret";
/// Password of the discoverable account.
pub const MULTI_NEW_PASSWORD: &str = "new-secret";

/// A mailing-list message delivered to both demo accounts (same
/// Message-ID): "All accounts" shows it once.
pub const MULTI_LIST_MESSAGE: &str = "From: Nail Users <users@lists.nail.example>\r\n\
To: users@lists.nail.example\r\n\
Subject: [nail-users] Release notes for M1\r\n\
Date: Tue, 29 Sep 2026 07:30:00 +0000\r\n\
Message-ID: <m1-release@lists.nail.example>\r\n\
List-Id: <users.lists.nail.example>\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
M1 brings IMAP IDLE, POP3, SMTP, search and unified inboxes.\r\n";

fn home_message(n: u32, subject: &str, body: &str) -> Vec<u8> {
    format!(
        "From: Grace Hopper <grace@home.example>\r\nTo: {MULTI_HOME_EMAIL}\r\n\
         Subject: {subject}\r\nDate: Mon, 28 Sep 2026 1{n}:00:00 +0000\r\n\
         Message-ID: <home-{n}@home.example>\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{body}\r\n"
    )
    .into_bytes()
}

fn autoconfig_xml(imap_port: u16, smtp_port: u16) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<clientConfig version="1.1">
  <emailProvider id="new.example">
    <domain>new.example</domain>
    <displayName>New Example Mail</displayName>
    <incomingServer type="imap">
      <hostname>127.0.0.1</hostname><port>{imap_port}</port><socketType>plain</socketType>
      <username>%EMAILADDRESS%</username><authentication>password-cleartext</authentication>
    </incomingServer>
    <outgoingServer type="smtp">
      <hostname>127.0.0.1</hostname><port>{smtp_port}</port><socketType>plain</socketType>
      <username>%EMAILADDRESS%</username><authentication>password-cleartext</authentication>
    </outgoingServer>
  </emailProvider>
</clientConfig>
"#
    )
}

/// The CalDAV source of `--seed demo-multi` (M3a; mock CalDAV server).
pub const MULTI_CALDAV_ID: &str = "work-calendars";
/// The CardDAV source of `--seed demo-multi` (same mock server).
pub const MULTI_CARDDAV_ID: &str = "work-contacts";
/// The ICS subscription of `--seed demo-multi` (the mock HTTP server).
pub const MULTI_HOLIDAYS_ID: &str = "holidays";
/// User name of the mock CalDAV / CardDAV server.
pub const MULTI_DAV_USER: &str = "ada";
/// The second principal of the mock CalDAV server (M3b): another person
/// whose free/busy the demo can ask for.
pub const MULTI_DAV_OTHER_USER: &str = "bob";
/// Address of [`MULTI_DAV_OTHER_USER`].
pub const MULTI_DAV_OTHER_EMAIL: &str = "bob@example.org";
/// Id of the Gmail API account of `--seed demo-multi` (M3b).
pub const MULTI_GMAIL_ID: &str = "gmail";
/// Address of the Gmail account (the mock Gmail server's demo mailbox).
pub const MULTI_GMAIL_EMAIL: &str = "ada@gmail.example";
/// The OAuth2 provider of the demo Gmail account (the mock authorization
/// server; no Google sign-in in demos).
pub const MULTI_GMAIL_PROVIDER: &str = "demo-google";
/// The refresh token the demo keychain holds for the Gmail account.
const MULTI_GMAIL_REFRESH_TOKEN: &str = "demo-gmail-refresh";
/// Password of the mock CalDAV / CardDAV server.
pub const MULTI_DAV_PASSWORD: &str = "dav-secret";
/// Id of the Exchange Online (Graph) account of `--seed demo-multi` (M3c).
pub const MULTI_EXCHANGE_ID: &str = "exchange";
/// Address of the Exchange Online account (the mock Graph server's tenant).
pub const MULTI_EXCHANGE_EMAIL: &str = "ada@contoso.example";
/// The OAuth2 provider of the demo Exchange Online account (the mock
/// authorization server; no Microsoft sign-in in demos).
pub const MULTI_MICROSOFT_PROVIDER: &str = "demo-microsoft";
/// The refresh token the demo keychain holds for the Exchange account.
const MULTI_EXCHANGE_REFRESH_TOKEN: &str = "demo-graph-refresh";
/// Id of the on-premises Exchange (EWS) account of `--seed demo-multi`
/// (M3c; switched off at first).
pub const MULTI_EWS_ID: &str = "corp";
/// Address of the on-premises Exchange (EWS) mailbox (Autodiscover:
/// `autodiscover.corp.example` points at the mock EWS server).
pub const MULTI_EWS_EMAIL: &str = "ada@corp.example";
/// Password of the mock EWS server (Basic over loopback).
pub const MULTI_EWS_PASSWORD: &str = "corp-secret";

/// An all-day event of a demo feed `days` after today.
fn feed_event(uid: &str, title: &str, days: i64) -> String {
    let day = chrono::Utc::now().date_naive() + chrono::Duration::days(days);
    let next = day + chrono::Duration::days(1);
    format!(
        "BEGIN:VEVENT\r\nUID:{uid}\r\nDTSTAMP:20260101T000000Z\r\nDTSTART;VALUE=DATE:{}\r\n\
         DTEND;VALUE=DATE:{}\r\nSUMMARY:{title}\r\nTRANSP:TRANSPARENT\r\nEND:VEVENT\r\n",
        day.format("%Y%m%d"),
        next.format("%Y%m%d")
    )
}

/// A demo ICS feed.
fn feed(name: &str, events: &[String]) -> Vec<u8> {
    format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Nail demo//EN\r\nX-WR-CALNAME:{name}\r\n{}END:VCALENDAR\r\n",
        events.concat()
    )
    .into_bytes()
}

/// A timed demo event `days` after today at `hour`:00 UTC for an hour,
/// weekly (12 times) when `weekly`.
fn dav_event(uid: &str, title: &str, days: i64, hour: u32, weekly: bool) -> String {
    let day = chrono::Utc::now().date_naive() + chrono::Duration::days(days);
    let stamp = |h: u32| format!("{}T{h:02}0000Z", day.format("%Y%m%d"));
    format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Nail demo//EN\r\nBEGIN:VEVENT\r\nUID:{uid}\r\n\
         DTSTAMP:20260101T000000Z\r\nDTSTART:{}\r\nDTEND:{}\r\nSUMMARY:{title}\r\n{}END:VEVENT\r\nEND:VCALENDAR\r\n",
        stamp(hour),
        stamp(hour + 1),
        if weekly {
            "RRULE:FREQ=WEEKLY;COUNT=12\r\n"
        } else {
            ""
        }
    )
}

/// A meeting the work identity organizes, with the second DAV principal
/// invited (M3b: the organizer panel, the cancel dialog).
fn dav_meeting(uid: &str, title: &str, days: i64, hour: u32) -> String {
    dav_event(uid, title, days, hour, false).replace(
        "END:VEVENT",
        &format!(
            "ORGANIZER;CN=Ada Lovelace:mailto:{MULTI_WORK_EMAIL}\r\n\
             ATTENDEE;CN=Ada Lovelace;PARTSTAT=ACCEPTED:mailto:{MULTI_WORK_EMAIL}\r\n\
             ATTENDEE;CN=Bob;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:{MULTI_DAV_OTHER_EMAIL}\r\n\
             END:VEVENT"
        ),
    )
}

/// A demo vCard 3.0.
fn dav_card(uid: &str, name: &str, email: &str) -> String {
    format!(
        "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:{uid}\r\nFN:{name}\r\nEMAIL;TYPE=INTERNET:{email}\r\nEND:VCARD\r\n"
    )
}

/// The running mock servers of `--seed demo-multi` and the settings that
/// point at them. Everything stops when this is dropped.
#[derive(Debug)]
pub struct DemoMulti {
    /// IMAP server of the "work" account (demo corpus, IDLE).
    pub work: MockImapServer,
    /// POP3 server of the "home" account.
    pub home: MockPop3Server,
    /// SMTP server for all accounts; records what it receives.
    pub smtp: MockSmtpServer,
    /// IMAP server of the discoverable account ([`MULTI_NEW_EMAIL`]).
    pub new_imap: MockImapServer,
    /// Autoconfig / ISPDB / Autodiscover answers.
    pub http: MockHttpServer,
    /// DNS answers (SRV / MX; empty zone).
    pub dns: MockDnsServer,
    /// ManageSieve server of the "work" account (M2c).
    pub sieve: MockSieveServer,
    /// CalDAV / CardDAV server of the PIM sources (M3a), with sample
    /// events and contacts; the second principal `bob` has busy time
    /// (M3b free/busy).
    pub dav: MockDavServer,
    /// The Gmail API server of the "gmail" account (M3b).
    pub gmail: MockGmailServer,
    /// The OAuth2 authorization server of the Gmail account (M3b) and the
    /// Exchange Online account (M3c).
    pub oauth: MockOAuthServer,
    /// The Microsoft Graph server of the "exchange" account (M3c).
    pub graph: MockGraphServer,
    /// The EWS server set up through Autodiscover (M3c).
    pub ews: MockEwsServer,
    /// Autodiscover for `contoso.example` (Exchange Online) and
    /// `corp.example` (the mock EWS server) (M3c).
    pub autodiscover: MockAutodiscoverServer,
    /// The clock snooze, quiet hours and notifications see; moved by the
    /// `clock-set` / `clock-advance` controls (M2c).
    pub clock: Arc<crate::AdjustableClock>,
    /// The in-memory keychain with the accounts' passwords.
    pub secrets: Arc<MemorySecretStore>,
    /// Discovery pointed at the mocks.
    pub discoverer: Arc<Discoverer>,
    pop3_counter: AtomicU32,
}

fn io_error(err: impl std::fmt::Display) -> std::io::Error {
    std::io::Error::other(err.to_string())
}

/// Starts the mock servers of `--seed demo-multi`.
pub async fn start_demo_multi() -> Result<DemoMulti> {
    let work_store = MailStore::demo();
    work_store.add_mailbox("Drafts", FolderRole::Drafts);
    work_store.add_mailbox("Trash", FolderRole::Trash);
    work_store.append("INBOX", MULTI_LIST_MESSAGE.as_bytes(), &[] as &[&str]);
    // M2c: a Junk folder, and a folder synced headers only (its bodies are
    // fetched on demand; the server search finds words of its bodies).
    work_store.add_mailbox("Junk", FolderRole::Junk);
    work_store.add_mailbox(MULTI_HEADERS_ONLY_FOLDER, FolderRole::Other);
    work_store.append(
        MULTI_HEADERS_ONLY_FOLDER,
        crlf(MULTI_NEWSLETTER).as_bytes(),
        &[] as &[&str],
    );
    let work = MockImapServer::start(
        MockImapConfig {
            username: MULTI_WORK_EMAIL.to_owned(),
            password: MULTI_WORK_PASSWORD.to_owned(),
            ..MockImapConfig::default()
        },
        work_store,
    )
    .await
    .map_err(io_error)?;

    let home = MockPop3Server::start(
        MockPop3Config {
            username: MULTI_HOME_EMAIL.to_owned(),
            password: MULTI_HOME_PASSWORD.to_owned(),
            ..MockPop3Config::default()
        },
        vec![
            Pop3Message {
                uid: "home-1".into(),
                data: home_message(1, "Garden party on Saturday", "Bring a salad."),
            },
            Pop3Message {
                uid: "home-2".into(),
                data: home_message(2, "Photos from the lake", "Here they are."),
            },
            Pop3Message {
                uid: "home-list".into(),
                data: MULTI_LIST_MESSAGE.as_bytes().to_vec(),
            },
        ],
    )
    .await
    .map_err(io_error)?;

    let smtp = MockSmtpServer::start(MockSmtpConfig {
        users: vec![
            (MULTI_WORK_EMAIL.to_owned(), MULTI_WORK_PASSWORD.to_owned()),
            (MULTI_HOME_EMAIL.to_owned(), MULTI_HOME_PASSWORD.to_owned()),
            (MULTI_NEW_EMAIL.to_owned(), MULTI_NEW_PASSWORD.to_owned()),
        ],
        ..MockSmtpConfig::default()
    })
    .await
    .map_err(io_error)?;

    let new_store = MailStore::new();
    new_store.add_mailbox("Sent", FolderRole::Sent);
    new_store.add_mailbox("Trash", FolderRole::Trash);
    new_store.append(
        "INBOX",
        format!(
            "From: New Example <welcome@new.example>\r\nTo: {MULTI_NEW_EMAIL}\r\n\
             Subject: Welcome to New Example Mail\r\nDate: Tue, 29 Sep 2026 08:00:00 +0000\r\n\
             Message-ID: <welcome@new.example>\r\n\r\nYour mailbox is ready.\r\n"
        )
        .as_bytes(),
        &[] as &[&str],
    );
    let new_imap = MockImapServer::start(
        MockImapConfig {
            username: MULTI_NEW_EMAIL.to_owned(),
            password: MULTI_NEW_PASSWORD.to_owned(),
            ..MockImapConfig::default()
        },
        new_store,
    )
    .await
    .map_err(io_error)?;

    let sieve = MockSieveServer::start(MockSieveConfig {
        username: MULTI_WORK_EMAIL.to_owned(),
        password: MULTI_WORK_PASSWORD.to_owned(),
        ..MockSieveConfig::default()
    })
    .await
    .map_err(io_error)?;

    let http = MockHttpServer::start().await.map_err(io_error)?;
    for path in ["/logo.png", "/pixel.png"] {
        http.get(
            Some(MULTI_IMAGES_HOST),
            path,
            MockResponse {
                status: 200,
                content_type: "image/png".to_owned(),
                body: PIXEL_PNG.to_vec(),
                headers: vec![("Set-Cookie".to_owned(), "tracker=1".to_owned())],
                delay: None,
            },
        );
    }
    http.get(
        Some("autoconfig.new.example"),
        "/mail/config-v1.1.xml",
        MockResponse::xml(autoconfig_xml(new_imap.port(), smtp.port())),
    );
    // `elsewhere.example` only has SRV records, pointing outside its domain:
    // setup shows them as unverified.
    let mut zone = DnsZone::new();
    zone.add_srv(
        "_imaps._tcp.elsewhere.example",
        0,
        0,
        993,
        "imap.mailhost.test",
    );
    let dns = MockDnsServer::start(zone).await.map_err(io_error)?;
    let client =
        nail_auth::http_client_builder(nail_auth::tls_config_with_roots(&[]).map_err(io_error)?)
            .no_proxy()
            .build()
            .map_err(io_error)?;
    let discoverer = Discoverer::new(
        Arc::new(ReqwestFetch::new(client).route_all_to(http.addr())),
        Arc::new(
            HickoryDns::with_name_server(dns.addr(), Duration::from_secs(2))
                .map_err(|e| io_error(e.0))?,
        ),
        Arc::new(TcpProbe::with_overrides(
            Duration::from_secs(2),
            HashMap::new(),
        )),
    )
    .with_timeout(Duration::from_secs(4))
    .with_guessing(false);

    // M3a: a CalDAV / CardDAV server with sample data, and ICS feeds.
    let dav = MockDavServer::start(MockDavConfig {
        username: MULTI_DAV_USER.to_owned(),
        password: MULTI_DAV_PASSWORD.to_owned(),
        // M3b: the work identity is the calendar user (free/busy
        // organizer), and a second person to ask about.
        email: MULTI_WORK_EMAIL.to_owned(),
        users: vec![MockDavUser::new(MULTI_DAV_OTHER_USER)],
        ..MockDavConfig::default()
    })
    .await
    .map_err(io_error)?;
    let bob = dav.user_calendar_path(MULTI_DAV_OTHER_USER, "personal");
    for (name, data) in [
        (
            "bob-workshop.ics",
            dav_event("bob-workshop@nail.example", "Workshop", 1, 10, false),
        ),
        (
            "bob-dentist.ics",
            dav_event("bob-dentist@nail.example", "Dentist", 1, 14, false),
        ),
    ] {
        dav.put_object(&bob, name, &data);
    }
    // M3b: the Gmail account (mock Gmail API + mock sign-in).
    let gmail = MockGmailServer::start_demo().await.map_err(io_error)?;
    let oauth = MockOAuthServer::start(MockOAuthConfig::default())
        .await
        .map_err(io_error)?;
    oauth.add_refresh_token(
        MULTI_GMAIL_REFRESH_TOKEN,
        MULTI_GMAIL_EMAIL,
        "https://mail.google.com/",
    );
    // M3c: Exchange Online (mock Graph, the same mock sign-in) and an
    // on-premises Exchange (mock EWS, Basic) found through Autodiscover.
    let graph = MockGraphServer::start_demo().await.map_err(io_error)?;
    oauth.add_refresh_token(
        MULTI_EXCHANGE_REFRESH_TOKEN,
        MULTI_EXCHANGE_EMAIL,
        "https://graph.microsoft.com/.default",
    );
    let ews = MockEwsServer::start_with(MockAuth::Basic {
        username: MULTI_EWS_EMAIL.to_owned(),
        password: MULTI_EWS_PASSWORD.to_owned(),
    })
    .await
    .map_err(io_error)?;
    let autodiscover = MockAutodiscoverServer::start_hosts(
        [
            (
                "autodiscover.contoso.example".to_owned(),
                MockHost {
                    v2: Some(MockV2::Url(
                        "https://outlook.office365.com/EWS/Exchange.asmx".to_owned(),
                    )),
                    ..MockHost::default()
                },
            ),
            (
                "autodiscover.corp.example".to_owned(),
                MockHost {
                    v2: Some(MockV2::Url(ews.url())),
                    ..MockHost::default()
                },
            ),
        ]
        .into_iter()
        .collect(),
    )
    .await
    .map_err(io_error)?;
    let personal = dav.calendar_path("personal");
    for (name, data) in [
        (
            "team-sync.ics",
            dav_event("team-sync@nail.example", "Team sync", 0, 9, true),
        ),
        (
            "quarterly-review.ics",
            dav_event("review@nail.example", "Quarterly review", 1, 14, false),
        ),
        (
            "project-kickoff.ics",
            dav_meeting("kickoff@nail.example", "Project kickoff", 2, 15),
        ),
    ] {
        dav.put_object(&personal, name, &data);
    }
    let book = dav.address_book_path("contacts");
    for (name, data) in [
        (
            "grace.vcf",
            dav_card("grace-hopper", "Grace Hopper", "grace@navy.example"),
        ),
        (
            "charles.vcf",
            dav_card(
                "charles-babbage",
                "Charles Babbage",
                "charles@engine.example",
            ),
        ),
    ] {
        dav.put_object(&book, name, &data);
    }
    let calendar = |body: Vec<u8>| MockResponse {
        status: 200,
        content_type: "text/calendar; charset=utf-8".to_owned(),
        body,
        headers: Vec::new(),
        delay: None,
    };
    http.get(
        None,
        "/holidays.ics",
        calendar(feed(
            "Holidays",
            &[
                feed_event("founders@holidays", "Founders' Day", 2),
                feed_event("harvest@holidays", "Harvest Festival", 9),
            ],
        )),
    );
    http.get(
        None,
        "/town.ics",
        calendar(feed(
            "Town events",
            &[feed_event("fair@town", "Town fair", 1)],
        )),
    );

    let secrets = Arc::new(MemorySecretStore::new());
    for source in [MULTI_CALDAV_ID, MULTI_CARDDAV_ID] {
        secrets
            .set(
                &SecretRef(format!("keychain:nail/pim/{source}/password")),
                MULTI_DAV_PASSWORD,
            )
            .map_err(io_error)?;
    }
    for (account, password) in [
        (MULTI_WORK_ID, MULTI_WORK_PASSWORD),
        (MULTI_HOME_ID, MULTI_HOME_PASSWORD),
    ] {
        secrets
            .set(
                &SecretRef::default_for(&AccountId::new(account), SecretKind::Password),
                password,
            )
            .map_err(io_error)?;
    }
    secrets
        .set(
            &SecretRef::default_for(&AccountId::new(MULTI_GMAIL_ID), SecretKind::OAuth2),
            MULTI_GMAIL_REFRESH_TOKEN,
        )
        .map_err(io_error)?;
    secrets
        .set(
            &SecretRef::default_for(&AccountId::new(MULTI_EXCHANGE_ID), SecretKind::OAuth2),
            MULTI_EXCHANGE_REFRESH_TOKEN,
        )
        .map_err(io_error)?;
    secrets
        .set(
            &SecretRef::default_for(&AccountId::new(MULTI_EWS_ID), SecretKind::Password),
            MULTI_EWS_PASSWORD,
        )
        .map_err(io_error)?;
    Ok(DemoMulti {
        work,
        home,
        smtp,
        new_imap,
        http,
        dns,
        sieve,
        dav,
        gmail,
        oauth,
        graph,
        ews,
        autodiscover,
        clock: Arc::new(crate::AdjustableClock::new()),
        secrets,
        discoverer: Arc::new(discoverer),
        pop3_counter: AtomicU32::new(0),
    })
}

/// DNS for the demo's Exchange Autodiscover: no SRV records (no sockets).
#[derive(Debug)]
struct NoSrv;

type DnsResult<T> = std::result::Result<Vec<T>, nail_discovery::NetError>;

impl nail_discovery::DnsLookup for NoSrv {
    fn srv<'a>(&'a self, _name: &'a str) -> BoxFuture<'a, DnsResult<nail_discovery::SrvRecord>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn mx<'a>(&'a self, _domain: &'a str) -> BoxFuture<'a, DnsResult<nail_discovery::MxRecord>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}

/// A mock Graph message as the controls report it.
fn graph_message_json(m: &nail_proto_graph::testing::MockMessage) -> Value {
    json!({
        "id": m.id,
        "folder": m.folder,
        "subject": subject_of(&m.raw),
        "read": m.is_read,
        "flag": m.flag,
        "categories": m.categories,
        "inference": m.inference,
    })
}

fn crlf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\n', "\r\n")
}

impl DemoMulti {
    /// `accounts.kdl` for the two demo accounts.
    pub fn accounts_kdl(&self) -> String {
        let (imap, pop3, smtp) = (self.work.port(), self.home.port(), self.smtp.port());
        let sieve = self.sieve.port();
        let (dav, http) = (self.dav.addr().port(), self.http.addr().port());
        let gmail = self.gmail.addr().port();
        let graph = self.graph.addr().port();
        let ews = self.ews.url();
        let (oauth_auth, oauth_token) = (self.oauth.authorize_url(), self.oauth.token_url());
        format!(
            r##"{MULTI_CONFIG_MARKER}
undo-send-delay 5

account "{MULTI_WORK_ID}" email="{MULTI_WORK_EMAIL}" name="Work" {{
    imap host="127.0.0.1" port={imap} tls="plaintext"
    smtp host="127.0.0.1" port={smtp} tls="plaintext"
    sieve host="127.0.0.1" port={sieve} tls="plaintext" enabled=#false
    sync {{
        folder path="{MULTI_HEADERS_ONLY_FOLDER}" bodies="headers-only"
    }}
    identity "{MULTI_WORK_EMAIL}" name="Ada Lovelace" {{
        crypto prefer-encrypt="mutual"
    }}
    authserv-id "{MULTI_AUTHSERV_ID}"
}}

account "{MULTI_HOME_ID}" email="{MULTI_HOME_EMAIL}" name="Home" {{
    pop3 host="127.0.0.1" port={pop3} tls="plaintext" leave-on-server=#true delete-when-deleted-locally=#true
    smtp host="127.0.0.1" port={smtp} tls="plaintext"
    identity "{MULTI_HOME_EMAIL}" name="Ada Lovelace" {{
        crypto prefer-encrypt="mutual"
    }}
    authserv-id "{MULTI_AUTHSERV_ID}"
}}

caldav "{MULTI_CALDAV_ID}" name="Work calendars" url="http://127.0.0.1:{dav}/" {{
    auth "password" username="{MULTI_DAV_USER}"
}}

carddav "{MULTI_CARDDAV_ID}" name="Work contacts" url="http://127.0.0.1:{dav}/" {{
    auth "password" username="{MULTI_DAV_USER}"
}}

calendar-subscription "{MULTI_HOLIDAYS_ID}" name="Holidays" url="http://127.0.0.1:{http}/holidays.ics" color="#d33"

oauth-provider "{MULTI_GMAIL_PROVIDER}" client-id="nail-test-client" auth-url="{oauth_auth}" token-url="{oauth_token}" {{
    scopes "https://mail.google.com/"
}}

// Switched off at first (the other demos count the unified inbox);
// Settings → Accounts → "Synchronize this account" turns it on.
account "{MULTI_GMAIL_ID}" email="{MULTI_GMAIL_EMAIL}" name="Gmail" enabled=#false {{
    gmail host="127.0.0.1" port={gmail} tls="plaintext" poll-interval=10 max-poll-interval=10 {{
        auth "oauth2" provider="{MULTI_GMAIL_PROVIDER}"
    }}
    identity "{MULTI_GMAIL_EMAIL}" name="Ada Lovelace"
}}

oauth-provider "{MULTI_MICROSOFT_PROVIDER}" client-id="nail-test-client" auth-url="{oauth_auth}" token-url="{oauth_token}" {{
    scopes "https://graph.microsoft.com/.default"
}}

// Exchange Online through Microsoft Graph (M3c); switched off at first
// like the Gmail account.
account "{MULTI_EXCHANGE_ID}" email="{MULTI_EXCHANGE_EMAIL}" name="Contoso" enabled=#false {{
    exchange host="127.0.0.1" port={graph} tls="plaintext" poll-interval=10 max-poll-interval=10 {{
        auth "oauth2" provider="{MULTI_MICROSOFT_PROVIDER}"
    }}
    identity "{MULTI_EXCHANGE_EMAIL}" name="Ada Lovelace"
}}

// Exchange Server on-premises through EWS (M3c): Basic over the loopback
// connection of the demo, streaming notifications; switched off at first.
account "{MULTI_EWS_ID}" email="{MULTI_EWS_EMAIL}" name="Corp" enabled=#false {{
    exchange engine="ews" host="127.0.0.1" url="{ews}" tls="plaintext" poll-interval=10 max-poll-interval=10 {{
        auth "password" username="{MULTI_EWS_EMAIL}"
    }}
    identity "{MULTI_EWS_EMAIL}" name="Ada Lovelace"
}}
"##
        )
    }

    /// Writes `accounts.kdl` into `config_dir` unless one exists.
    pub fn write_config(&self, config_dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(config_dir)?;
        let path = config_dir.join(crate::accounts::ACCOUNTS_FILE);
        if !path.exists() {
            std::fs::write(path, self.accounts_kdl())?;
        }
        Ok(())
    }

    /// `options` pointed at the mocks: in-memory secrets, mock discovery,
    /// short reconnect delays, the test trust anchor, offline key lookups.
    pub fn service_options(&self, mut options: crate::ServiceOptions) -> crate::ServiceOptions {
        options.secrets = Some(Arc::clone(&self.secrets) as Arc<dyn SecretStore>);
        options.proxy_fetcher = self.proxy_fetcher();
        options.discoverer = Some(Arc::clone(&self.discoverer));
        options.timing = crate::EngineTiming::fast();
        options.open_browser = false;
        options.clock = Some(Arc::clone(&self.clock) as Arc<dyn nail_triage::Clock>);
        options.exchange = self.exchange_hooks();
        demo_options(options)
    }

    /// Exchange Autodiscover and Graph pointed at the mocks (M3c).
    fn exchange_hooks(&self) -> crate::ExchangeHooks {
        let http = nail_auth::tls_config_with_roots(&[])
            .ok()
            .and_then(|tls| nail_auth::http_client_builder(tls).no_proxy().build().ok())
            .map(|client| {
                Arc::new(
                    nail_exchange::http::ReqwestHttp::new(client)
                        .route_all_to(self.autodiscover.addr()),
                ) as Arc<dyn nail_exchange::http::ExchangeHttp>
            });
        crate::ExchangeHooks {
            autodiscover_http: http,
            autodiscover_dns: Some(Arc::new(NoSrv)),
            no_microsoft_lookup: true,
            negotiate: None,
            graph_endpoint: Some(crate::GraphEndpoint {
                host: "127.0.0.1".to_owned(),
                port: self.graph.addr().port(),
                tls: nail_api::accounts::TlsMode::Plaintext,
                provider: MULTI_MICROSOFT_PROVIDER.to_owned(),
            }),
        }
    }

    /// Gives both demo accounts an own OpenPGP key (Ed25519 + X25519,
    /// generated, protected by the in-memory keychain) and an own S/MIME
    /// certificate (`ada-work.p12` / `ada-home.p12` of the corpus, issued
    /// by the test CA). Keys that exist already are kept.
    pub async fn seed_security(&self, service: &crate::Service) -> Result<()> {
        service
            .blocking(|s| {
                for (email, p12) in [
                    (MULTI_WORK_EMAIL, "ada-work.p12"),
                    (MULTI_HOME_EMAIL, "ada-home.p12"),
                ] {
                    let pgp = s.pgp();
                    if pgp.own_key_for(email, None).is_err() {
                        pgp.generate(
                            "Ada Lovelace",
                            email,
                            KeyAlgorithm::Curve25519,
                            None,
                            &SecretProtection::Keychain,
                        )?;
                    }
                    let smime = s.smime();
                    if smime.signer(email, None).is_err()
                        && let Some(data) = crypto_fixture(CryptoFixtures::Smime, p12)
                    {
                        smime.import_pkcs12(
                            &data,
                            DEMO_PKCS12_PASSWORD,
                            &SecretProtection::Keychain,
                        )?;
                    }
                }
                Ok(())
            })
            .await?;
        tracing::info!("demo keys and certificates ready");
        Ok(())
    }

    /// The proxy's fetcher: [`MULTI_IMAGES_HOST`] is the mock HTTP server
    /// (loopback, allowed for its port only), [`MULTI_PRIVATE_HOST`] a
    /// private address; every other name is unknown.
    fn proxy_fetcher(&self) -> Option<nail_proxy::Fetcher> {
        let resolver = nail_proxy::testing::ScriptedResolver::new();
        resolver.set(MULTI_IMAGES_HOST, &["127.0.0.1"]);
        resolver.set(MULTI_PRIVATE_HOST, &["10.0.0.7"]);
        let tls = nail_auth::tls_config().ok()?;
        Some(
            nail_proxy::Fetcher::new(Arc::new(tls))
                .with_resolver(resolver)
                .allow_loopback_port_for_tests(self.http.addr().port()),
        )
    }

    /// The mock DAV path of a collection: `personal`, `tasks`, `holidays`
    /// (calendars) or `contacts` (address book).
    fn dav_path(&self, collection: &str) -> std::result::Result<String, String> {
        match collection {
            "personal" | "tasks" | "holidays" => Ok(self.dav.calendar_path(collection)),
            "contacts" => Ok(self.dav.address_book_path(collection)),
            other => Err(format!("no mock DAV collection {other:?}")),
        }
    }

    /// The mock Gmail id of the message named by `id` or by a `subject`
    /// it contains.
    fn gmail_message_id(&self, args: &Value) -> std::result::Result<String, String> {
        if let Some(id) = args.get("id").and_then(Value::as_str) {
            return Ok(id.to_owned());
        }
        let subject = args
            .get("subject")
            .and_then(Value::as_str)
            .ok_or("id or subject is required")?;
        self.gmail
            .with_mailbox(|m| {
                m.message_ids().into_iter().find(|id| {
                    m.message(id)
                        .is_some_and(|msg| subject_of(&msg.raw).contains(subject))
                })
            })
            .ok_or_else(|| format!("no Gmail message {subject:?}"))
    }

    fn imap(&self, account: &str) -> std::result::Result<&MockImapServer, String> {
        match account {
            MULTI_WORK_ID => Ok(&self.work),
            "new" | "bob" => Ok(&self.new_imap),
            other => Err(format!("no mock IMAP server for account {other:?}")),
        }
    }

    /// Runs one test-control command (the `nail serve --seed demo-multi`
    /// control endpoint). Commands and their JSON arguments:
    ///
    /// * `imap-deliver {account?, mailbox?, raw, flags?}` → `{uid}`
    /// * `imap-flags {account?, mailbox?, subject, flags}` replaces the
    ///   flags of the first message whose subject contains `subject`
    /// * `imap-messages {account?, mailbox?}` → `[{uid, subject, flags}]`
    /// * `imap-mailboxes {account?}` → `[{name, count}]`
    /// * `imap-offline {account?, offline}`
    /// * `pop3-deliver {raw, uid?}` → `{uid}`
    /// * `pop3-state` → `{uids, deleted}`
    /// * `smtp-messages` → `[{from, to, data}]`
    /// * `remote-content` → `{images, private, requests}`: base URLs of the
    ///   made-up image hosts and the paths the image host was asked for
    /// * `sieve-scripts` → `{active, scripts: [{name, active, content}],
    ///   uploads}` of the ManageSieve server (M2c)
    /// * `dav-objects {collection?}` → `[{name, etag, data}]` of the mock
    ///   CalDAV / CardDAV server (`personal` (default), `tasks`,
    ///   `holidays`, `contacts`); `dav-put {collection?, name, data}` →
    ///   `{etag}` and `dav-delete {collection?, name}` change it as another
    ///   client would (M3a)
    /// * `feeds` → `{holidays, town}`: URLs of the demo ICS feeds (M3a)
    /// * `gmail-deliver {raw, labels?}` → `{id}` (default labels `INBOX`,
    ///   `UNREAD`); `gmail-message {id?, subject?}` → `{id, labels}`;
    ///   `gmail-labels` → `[{id, name}]`; `gmail-sent` → `[data]`;
    ///   `gmail-modify {id?, subject?, add?, remove?}`: the mock Gmail
    ///   server of the "gmail" account (M3b)
    /// * `dav-user-put {user?, name, data}` → `{etag}`: an event in the
    ///   `personal` calendar of the second principal (`bob`, M3b)
    /// * `graph-deliver {raw, folder?}` → `{id}`; `graph-messages
    ///   {mailbox?}` / `graph-folders {mailbox?}` → the mock Graph tenant's
    ///   messages / folders; `graph-sent` → `[data]`; `graph-settings` →
    ///   `{categories, automatic_replies, rules}`; `graph-pim` → `{events,
    ///   tasks, contacts}`; `graph-meetings` → `[{event_id, action}]`: the
    ///   "exchange" account's server (M3c)
    /// * `ews-deliver {raw, folder?}` → `{id}`; `ews-messages {folder?}` →
    ///   `[{id, subject, read}]`; `ews-sent` → `[data]`: the mock EWS
    ///   server Autodiscover finds for `corp.example` (M3c)
    /// * `clock` → `{now}`; `clock-set {at}` (Unix seconds);
    ///   `clock-advance {seconds}`: the clock of snooze, quiet hours and
    ///   notifications (M2c)
    ///
    /// `account` defaults to the IMAP account `work` (`new` is the
    /// discoverable one), `mailbox` to `INBOX`.
    pub fn control(&self, command: &str, args: &Value) -> std::result::Result<Value, String> {
        let text = |key: &str| args.get(key).and_then(Value::as_str);
        let account = text("account").unwrap_or(MULTI_WORK_ID);
        let mailbox = text("mailbox").unwrap_or("INBOX");
        let flags: Vec<String> = args
            .get("flags")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|f| f.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        match command {
            "imap-deliver" => {
                let raw = text("raw").ok_or("raw is required")?;
                let uid = self
                    .imap(account)?
                    .store()
                    .append(mailbox, crlf(raw).as_bytes(), &flags)
                    .ok_or_else(|| format!("no mailbox {mailbox}"))?;
                Ok(json!({ "uid": uid }))
            }
            "imap-flags" => {
                let subject = text("subject").ok_or("subject is required")?;
                let server = self.imap(account)?;
                let uid = find_by_subject(server.store(), mailbox, subject)?;
                server.store().set_flags(mailbox, uid, &flags);
                Ok(json!({ "uid": uid }))
            }
            "imap-messages" => {
                let server = self.imap(account)?;
                let found = server
                    .store()
                    .mailbox(mailbox)
                    .ok_or_else(|| format!("no mailbox {mailbox}"))?;
                Ok(Value::Array(
                    found
                        .messages
                        .iter()
                        .map(|m| {
                            json!({
                                "uid": m.uid,
                                "subject": subject_of(&m.raw),
                                "flags": m.flags,
                            })
                        })
                        .collect(),
                ))
            }
            "imap-mailboxes" => Ok(Value::Array(
                self.imap(account)?
                    .store()
                    .mailboxes()
                    .iter()
                    .map(|m| json!({ "name": m.name, "count": m.messages.len() }))
                    .collect(),
            )),
            "imap-offline" => {
                let offline = args
                    .get("offline")
                    .and_then(Value::as_bool)
                    .ok_or("offline is required")?;
                self.imap(account)?.set_offline(offline);
                Ok(json!({ "offline": offline }))
            }
            "pop3-deliver" => {
                let raw = text("raw").ok_or("raw is required")?;
                let n = self.pop3_counter.fetch_add(1, Ordering::Relaxed) + 1;
                let uid = text("uid").map_or_else(|| format!("control-{n}"), str::to_owned);
                self.home.add_message(&uid, crlf(raw));
                Ok(json!({ "uid": uid }))
            }
            "pop3-state" => Ok(json!({
                "uids": self.home.uids(),
                "deleted": self.home.deleted(),
            })),
            "smtp-messages" => Ok(Value::Array(
                self.smtp
                    .messages()
                    .iter()
                    .map(|m| {
                        json!({
                            "from": m.mail_from,
                            "to": m.recipient_addresses(),
                            "data": m.text(),
                        })
                    })
                    .collect(),
            )),
            "remote-content" => {
                let port = self.http.addr().port();
                let requests: Vec<Value> = self
                    .http
                    .requests()
                    .into_iter()
                    .filter(|r| r.host == MULTI_IMAGES_HOST)
                    .map(|r| json!(r.path))
                    .collect();
                Ok(json!({
                    "images": format!("http://{MULTI_IMAGES_HOST}:{port}"),
                    "private": format!("http://{MULTI_PRIVATE_HOST}:{port}"),
                    "requests": requests,
                }))
            }
            "sieve-scripts" => Ok(json!({
                "active": self.sieve.active(),
                "uploads": self.sieve.uploads().len(),
                "scripts": self
                    .sieve
                    .scripts()
                    .into_iter()
                    .map(|s| json!({ "name": s.name, "active": s.active, "content": s.content }))
                    .collect::<Vec<_>>(),
            })),
            "dav-objects" => {
                let path = self.dav_path(text("collection").unwrap_or("personal"))?;
                let collection = self
                    .dav
                    .collection(&path)
                    .ok_or_else(|| format!("no collection {path}"))?;
                Ok(Value::Array(
                    collection
                        .objects
                        .iter()
                        .map(|(name, o)| json!({ "name": name, "etag": o.etag, "data": o.data }))
                        .collect(),
                ))
            }
            "dav-put" => {
                let path = self.dav_path(text("collection").unwrap_or("personal"))?;
                let name = text("name").ok_or("name is required")?;
                let data = text("data").ok_or("data is required")?;
                let etag = self
                    .dav
                    .put_object(&path, name, &crlf(data))
                    .ok_or_else(|| format!("no collection {path}"))?;
                Ok(json!({ "etag": etag }))
            }
            "dav-delete" => {
                let path = self.dav_path(text("collection").unwrap_or("personal"))?;
                let name = text("name").ok_or("name is required")?;
                Ok(json!({ "deleted": self.dav.delete_object(&path, name) }))
            }
            "gmail-deliver" => {
                let raw = text("raw").ok_or("raw is required")?;
                let labels: Vec<String> = args
                    .get("labels")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(|l| l.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_else(|| vec!["INBOX".to_owned(), "UNREAD".to_owned()]);
                let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
                Ok(json!({ "id": self.gmail.deliver(crlf(raw).as_bytes(), &labels) }))
            }
            "gmail-message" => {
                let id = self.gmail_message_id(args)?;
                Ok(json!({ "id": id, "labels": self.gmail.labels_of(&id) }))
            }
            "gmail-modify" => {
                let id = self.gmail_message_id(args)?;
                let list = |key: &str| -> Vec<String> {
                    args.get(key)
                        .and_then(Value::as_array)
                        .map(|a| {
                            a.iter()
                                .filter_map(|l| l.as_str().map(str::to_owned))
                                .collect()
                        })
                        .unwrap_or_default()
                };
                let (add, remove) = (list("add"), list("remove"));
                let add: Vec<&str> = add.iter().map(String::as_str).collect();
                let remove: Vec<&str> = remove.iter().map(String::as_str).collect();
                self.gmail.modify(&id, &add, &remove);
                Ok(json!({ "id": id, "labels": self.gmail.labels_of(&id) }))
            }
            "gmail-labels" => Ok(Value::Array(
                self.gmail
                    .labels()
                    .into_iter()
                    .map(|l| json!({ "id": l.id, "name": l.name }))
                    .collect(),
            )),
            "gmail-sent" => Ok(Value::Array(
                self.gmail
                    .sent()
                    .iter()
                    .map(|raw| json!(String::from_utf8_lossy(raw)))
                    .collect(),
            )),
            "dav-user-put" => {
                let user = text("user").unwrap_or(MULTI_DAV_OTHER_USER);
                let name = text("name").ok_or("name is required")?;
                let data = text("data").ok_or("data is required")?;
                let path = self.dav.user_calendar_path(user, "personal");
                let etag = self
                    .dav
                    .put_object(&path, name, &crlf(data))
                    .ok_or_else(|| format!("no calendar of {user}"))?;
                Ok(json!({ "etag": etag }))
            }
            "feeds" => {
                let base = self.http.base_url();
                Ok(json!({
                    "holidays": format!("{base}/holidays.ics"),
                    "town": format!("{base}/town.ics"),
                }))
            }
            "clock" => Ok(json!({ "now": nail_triage::Clock::unix(&*self.clock) })),
            "clock-set" => {
                let at = args
                    .get("at")
                    .and_then(Value::as_i64)
                    .ok_or("at (Unix seconds) is required")?;
                self.clock.set(at);
                Ok(json!({ "now": nail_triage::Clock::unix(&*self.clock) }))
            }
            "clock-advance" => {
                let seconds = args
                    .get("seconds")
                    .and_then(Value::as_i64)
                    .ok_or("seconds is required")?;
                self.clock.advance(seconds);
                Ok(json!({ "now": nail_triage::Clock::unix(&*self.clock) }))
            }
            // M3c: the mock Graph server of the "exchange" account and the
            // mock EWS server.
            "graph-deliver" => {
                let raw = text("raw").ok_or("raw is required")?;
                let folder = text("folder").unwrap_or("inbox");
                let id = self.graph.deliver(None, folder, crlf(raw).as_bytes());
                Ok(json!({ "id": id }))
            }
            "graph-messages" => Ok(self.graph.with_mailbox(text("mailbox"), |m, _| {
                Value::Array(m.messages.values().map(graph_message_json).collect())
            })),
            "graph-folders" => Ok(self.graph.with_mailbox(text("mailbox"), |m, _| {
                Value::Array(
                    m.folders
                        .values()
                        .map(|f| json!({"id": f.id, "name": f.name, "well_known": f.well_known}))
                        .collect(),
                )
            })),
            "graph-sent" => Ok(Value::Array(
                self.graph
                    .sent()
                    .into_iter()
                    .map(|s| Value::String(String::from_utf8_lossy(&s.raw).into_owned()))
                    .collect(),
            )),
            "graph-settings" => Ok(self.graph.with_mailbox(None, |m, _| {
                json!({
                    "categories": m.categories,
                    "automatic_replies": m.automatic_replies,
                    "rules": m.rules,
                })
            })),
            "graph-pim" => Ok(self.graph.with_mailbox(None, |m, _| {
                json!({
                    "events": m.events.values().collect::<Vec<_>>(),
                    "tasks": m
                        .tasks
                        .values()
                        .map(|t| json!({"list": t.list, "task": t.value, "checklist": t.checklist}))
                        .collect::<Vec<_>>(),
                    "contacts": m.contacts.values().collect::<Vec<_>>(),
                })
            })),
            "graph-meetings" => Ok(Value::Array(
                self.graph
                    .meeting_actions()
                    .into_iter()
                    .map(|a| json!({"event_id": a.event_id, "action": a.action}))
                    .collect(),
            )),
            "ews-deliver" => {
                let raw = text("raw").ok_or("raw is required")?;
                let folder = self
                    .ews
                    .folder_id(None, text("folder").unwrap_or("inbox"))
                    .ok_or("no such EWS folder")?;
                let id = self.ews.deliver(&folder, crlf(raw).as_bytes(), false);
                Ok(json!({ "id": id }))
            }
            "ews-messages" => {
                let folder = self
                    .ews
                    .folder_id(None, text("folder").unwrap_or("inbox"))
                    .ok_or("no such EWS folder")?;
                Ok(Value::Array(
                    self.ews
                        .items_in(&folder)
                        .iter()
                        .map(|i| {
                            json!({
                                "id": i.id,
                                "subject": i.text("Subject"),
                                "read": i.text("IsRead") == Some("true"),
                            })
                        })
                        .collect(),
                ))
            }
            "ews-sent" => Ok(Value::Array(
                self.ews
                    .sent()
                    .into_iter()
                    .map(|s| Value::String(String::from_utf8_lossy(&s.raw).into_owned()))
                    .collect(),
            )),
            other => Err(format!("unknown control command {other:?}")),
        }
    }
}

fn subject_of(raw: &[u8]) -> String {
    nail_mime::parse_message(raw).headers.subject
}

fn find_by_subject(
    store: &MailStore,
    mailbox: &str,
    subject: &str,
) -> std::result::Result<u32, String> {
    store
        .mailbox(mailbox)
        .ok_or_else(|| format!("no mailbox {mailbox}"))?
        .messages
        .iter()
        .find(|m| subject_of(&m.raw).contains(subject))
        .map(|m| m.uid)
        .ok_or_else(|| format!("no message with subject {subject:?} in {mailbox}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::FolderRole;

    #[test]
    fn seeds_the_corpus_idempotently() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_in_memory().unwrap();
        let blobs = BlobStore::open(dir.path().join("blobs")).unwrap();

        let seed = seed_demo(&store, &blobs).unwrap();
        let corpus = demo_corpus();
        let expected: usize = corpus.iter().map(|f| f.messages.len()).sum();
        assert_eq!(seed.messages, expected);
        assert_eq!(seed.folders[0].role, FolderRole::Inbox);
        assert_eq!(
            seed.folders.iter().map(|f| f.total as usize).sum::<usize>(),
            expected
        );

        let again = seed_demo(&store, &blobs).unwrap();
        assert_eq!(again, seed);
        assert_eq!(store.list_accounts().unwrap(), vec![seed.account]);
    }
}
