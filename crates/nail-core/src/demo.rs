//! Demo data and in-process mock servers, for development and Playwright:
//!
//! * `nail serve --seed demo`: the `nail-testkit` corpus as a local account;
//! * `--seed demo-imap`: the corpus on a mock IMAP server, synced like a
//!   real account;
//! * `--seed demo-multi` ([`start_demo_multi`]): two configured accounts
//!   (IMAP "work" with IDLE, POP3 "home"), one SMTP server for both, and a
//!   third IMAP server plus autoconfig / DNS mocks for setting up a new
//!   account by discovery. Secrets live in memory, never in the OS
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
use nail_pgp::discovery::{HttpClient, HttpResponse};
use nail_proto_imap::{Credentials, ImapConfig, Security};
use nail_store::{BlobStore, Store};
use nail_testkit::{
    CryptoFixtures, MailStore, MockImapConfig, MockImapServer, crypto_fixture, demo_corpus,
};
use nail_testkit_net::{
    DnsZone, MockDnsServer, MockHttpServer, MockPop3Config, MockPop3Server, MockResponse,
    MockSmtpConfig, MockSmtpServer, Pop3Message,
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

    let secrets = Arc::new(MemorySecretStore::new());
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
    Ok(DemoMulti {
        work,
        home,
        smtp,
        new_imap,
        http,
        dns,
        secrets,
        discoverer: Arc::new(discoverer),
        pop3_counter: AtomicU32::new(0),
    })
}

fn crlf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\n', "\r\n")
}

impl DemoMulti {
    /// `accounts.kdl` for the two demo accounts.
    pub fn accounts_kdl(&self) -> String {
        let (imap, pop3, smtp) = (self.work.port(), self.home.port(), self.smtp.port());
        format!(
            r#"{MULTI_CONFIG_MARKER}
undo-send-delay 5

account "{MULTI_WORK_ID}" email="{MULTI_WORK_EMAIL}" name="Work" {{
    imap host="127.0.0.1" port={imap} tls="plaintext"
    smtp host="127.0.0.1" port={smtp} tls="plaintext"
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
"#
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
        demo_options(options)
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
