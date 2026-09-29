//! M2a (ADR 0034): end-to-end encryption and data protection in the core,
//! against the `demo-multi` mock servers: OpenPGP and S/MIME round trips
//! between the two demo accounts (sent over the mock SMTP server,
//! delivered to the POP3 account), header protection on the wire, Bcc
//! copies, tampered signatures, Autocrypt learning a key from one message,
//! sender authentication and link warnings, key management requests, the
//! Autocrypt setup message, decrypted indexing and encryption at rest.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use nail_api::accounts::ComposerMode;
use nail_api::engine::{BoxFuture, EngineError};
use nail_api::security::{
    AuthVerdict, AutocryptRecommendation, ChainState, CryptoTechnology, DecryptionState,
    HeaderProtection, KeyEncoding, KeyFilter, KeyLookupSource, KeySource, MessageWarning,
    SecuritySettings, SendCrypto, SignatureState, TrustDecision, TrustLevel, VaultState,
};
use nail_api::{
    AccountId, Address, Draft, ErrorCode, Folder, FolderRole, MessageBody, MessageSummary, Request,
    Response, SearchQuery,
};
use nail_core::demo::{
    DemoMulti, MULTI_HOME_EMAIL, MULTI_HOME_ID, MULTI_WORK_EMAIL, MULTI_WORK_ID, start_demo_multi,
};
use nail_core::{Service, ServiceOptions};
use nail_pgp::discovery::{HttpClient, HttpResponse};
use nail_testkit::{CryptoFixtures, crypto_fixture};
use serde_json::json;
use tempfile::TempDir;

const TIMEOUT: Duration = Duration::from_secs(20);

struct Fixture {
    service: Service,
    demo: DemoMulti,
    _config_dir: TempDir,
    data_dir: TempDir,
}

async fn setup() -> Fixture {
    let demo = start_demo_multi().await.unwrap();
    let data_dir = tempfile::tempdir().unwrap();
    let config_dir = tempfile::tempdir().unwrap();
    demo.write_config(config_dir.path()).unwrap();
    let options = demo.service_options(ServiceOptions::new(data_dir.path(), config_dir.path()));
    let service = Service::open(options).unwrap();
    demo.seed_security(&service).await.unwrap();
    let f = Fixture {
        service,
        demo,
        _config_dir: config_dir,
        data_dir,
    };
    eventually("both accounts synced", || async {
        inbox(&f, MULTI_WORK_ID).await.len() == 15 && inbox(&f, MULTI_HOME_ID).await.len() == 3
    })
    .await;
    f
}

async fn eventually<F, Fut>(what: &str, mut check: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    loop {
        if check().await {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for: {what}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn request(f: &Fixture, request: Request) -> Response {
    f.service.handle(request).await
}

fn control(f: &Fixture, command: &str, args: serde_json::Value) -> serde_json::Value {
    f.demo.control(command, &args).unwrap()
}

async fn folder(f: &Fixture, account: &str, role: FolderRole) -> Option<Folder> {
    match request(
        f,
        Request::ListFolders {
            account_id: AccountId::new(account),
        },
    )
    .await
    {
        Response::Folders { folders } => folders.into_iter().find(|x| x.role == role),
        other => panic!("unexpected {other:?}"),
    }
}

async fn list(f: &Fixture, account: &str, role: FolderRole) -> Vec<MessageSummary> {
    let Some(folder) = folder(f, account, role).await else {
        return Vec::new();
    };
    match request(
        f,
        Request::ListMessages {
            folder_id: folder.id,
            offset: 0,
            limit: 500,
        },
    )
    .await
    {
        Response::Messages { messages, .. } => messages,
        other => panic!("unexpected {other:?}"),
    }
}

async fn inbox(f: &Fixture, account: &str) -> Vec<MessageSummary> {
    list(f, account, FolderRole::Inbox).await
}

async fn body(f: &Fixture, message: &MessageSummary) -> MessageBody {
    match request(
        f,
        Request::GetMessage {
            id: message.id.clone(),
        },
    )
    .await
    {
        Response::Message { message } => *message,
        other => panic!("unexpected {other:?}"),
    }
}

fn draft(to: &str, subject: &str, text: &str) -> Draft {
    Draft {
        id: None,
        identity_id: MULTI_WORK_EMAIL.into(),
        to: vec![Address {
            name: None,
            email: to.into(),
        }],
        cc: vec![],
        bcc: vec![],
        subject: subject.into(),
        mode: ComposerMode::Plain,
        text: text.into(),
        html: None,
        markdown: None,
        attachments: vec![],
        context: None,
        signature: nail_api::SignatureChoice::Identity,
        markdown_part: false,
        automatic: None,
        crypto: None,
    }
}

/// Saves and sends a draft; returns the send response.
async fn send(f: &Fixture, draft: Draft, crypto: Option<SendCrypto>) -> Response {
    let Response::DraftSaved { draft_id } = request(f, Request::SaveDraft { draft }).await else {
        panic!("draft not saved")
    };
    request(
        f,
        Request::SendDraft {
            draft_id,
            delay_secs: Some(0),
            send_at: None,
            crypto,
        },
    )
    .await
}

/// What the SMTP server received: `(envelope recipients, data)`.
fn received(f: &Fixture) -> Vec<(Vec<String>, String)> {
    control(f, "smtp-messages", json!({}))
        .as_array()
        .unwrap()
        .iter()
        .map(|m| {
            (
                m["to"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|t| t.as_str().unwrap().to_owned())
                    .collect(),
                m["data"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

/// Waits until the SMTP server got `count` messages in total.
async fn received_count(f: &Fixture, count: usize) -> Vec<(Vec<String>, String)> {
    eventually("the SMTP server received the mail", || async {
        received(f).len() >= count
    })
    .await;
    received(f)
}

/// Delivers raw mail to the POP3 "home" account and waits for it.
async fn deliver_home(f: &Fixture, raw: &str) -> MessageSummary {
    let before: Vec<_> = inbox(f, MULTI_HOME_ID)
        .await
        .into_iter()
        .map(|m| m.id)
        .collect();
    control(f, "pop3-deliver", json!({ "raw": raw }));
    request(
        f,
        Request::SyncNow {
            account_id: Some(AccountId::new(MULTI_HOME_ID)),
        },
    )
    .await;
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    loop {
        if let Some(new) = inbox(f, MULTI_HOME_ID)
            .await
            .into_iter()
            .find(|m| !before.contains(&m.id))
        {
            return new;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "POP3 mail not downloaded"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Delivers raw mail to the IMAP "work" account's INBOX and waits for it.
async fn deliver_work(f: &Fixture, raw: &str, subject: &str) -> MessageSummary {
    control(f, "imap-deliver", json!({ "raw": raw }));
    request(
        f,
        Request::SyncNow {
            account_id: Some(AccountId::new(MULTI_WORK_ID)),
        },
    )
    .await;
    let mut found = None;
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    while found.is_none() {
        found = inbox(f, MULTI_WORK_ID)
            .await
            .into_iter()
            .find(|m| m.subject == subject);
        assert!(
            tokio::time::Instant::now() < deadline,
            "{subject} not synced"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    found.unwrap()
}

fn signed_encrypted() -> SendCrypto {
    SendCrypto {
        sign: true,
        encrypt: true,
        technology: None,
        protect_headers: None,
    }
}

#[tokio::test]
async fn openpgp_round_trip_with_header_protection() {
    let f = setup().await;
    let sent = send(
        &f,
        draft(
            MULTI_HOME_EMAIL,
            "Secret plans",
            "The rocket leaves at dawn.",
        ),
        Some(signed_encrypted()),
    )
    .await;
    assert!(matches!(sent, Response::SendQueued { .. }), "{sent:?}");
    let mail = received_count(&f, 1).await;
    let (to, data) = &mail[0];
    assert_eq!(to, &[MULTI_HOME_EMAIL.to_owned()]);
    // On the wire: PGP/MIME, the subject hidden, the body unreadable, the
    // sender's Autocrypt key attached.
    assert!(data.contains("multipart/encrypted"), "{data}");
    assert!(data.contains("Subject: ..."), "{data}");
    assert!(!data.contains("Secret plans"), "{data}");
    assert!(!data.contains("rocket"), "{data}");
    assert!(data.contains("Autocrypt: addr=ada@work.example"), "{data}");
    assert!(data.contains("prefer-encrypt=mutual"), "{data}");

    // The home account reads it: decrypted, the signature valid, the real
    // subject back from the protected headers.
    let message = deliver_home(&f, data).await;
    assert_eq!(message.subject, "...");
    let shown = body(&f, &message).await;
    let security = shown.security.clone().expect("a security status");
    assert_eq!(security.technology, CryptoTechnology::OpenPgp);
    assert_eq!(
        security.encryption.as_ref().map(|e| e.state),
        Some(DecryptionState::Decrypted)
    );
    assert_eq!(security.signatures.len(), 1);
    assert_eq!(security.signatures[0].state, SignatureState::Valid);
    assert!(security.signatures[0].signer_matches_from);
    assert_eq!(security.signatures[0].trust, TrustLevel::Own);
    assert_eq!(security.header_protection, HeaderProtection::Protected);
    assert_eq!(shown.headers.subject, "Secret plans");
    assert!(shown.text.contains("rocket"));

    // Only the findings are cached, never the decrypted text.
    let cached = f
        .service
        .store()
        .message_security(&message.id)
        .unwrap()
        .expect("cached");
    assert_eq!(cached.status, shown.security);
    let blob_dir = f.data_dir.path().join("nail.sqlite3");
    let db = std::fs::read(&blob_dir).unwrap();
    assert!(!db.windows(6).any(|w| w == b"rocket"));

    // The Sent copy (encrypted to the sender too) reads the same.
    eventually("the Sent copy", || async {
        !list(&f, MULTI_WORK_ID, FolderRole::Sent).await.is_empty()
    })
    .await;
    let copy = list(&f, MULTI_WORK_ID, FolderRole::Sent).await.remove(0);
    let copy = body(&f, &copy).await;
    assert_eq!(copy.headers.subject, "Secret plans");
    assert_eq!(
        copy.security.unwrap().encryption.unwrap().state,
        DecryptionState::Decrypted
    );
    f.service.shutdown().await;
}

#[tokio::test]
async fn tampered_openpgp_signature_is_invalid() {
    let f = setup().await;
    let sign_only = SendCrypto {
        sign: true,
        ..SendCrypto::none()
    };
    send(
        &f,
        draft(MULTI_HOME_EMAIL, "Signed note", "Pay 10 EUR to Grace."),
        Some(sign_only),
    )
    .await;
    let mail = received_count(&f, 1).await;
    let data = &mail[0].1;
    assert!(data.contains("multipart/signed"), "{data}");
    let good = deliver_home(&f, data).await;
    let shown = body(&f, &good).await;
    assert_eq!(
        shown.security.unwrap().signatures[0].state,
        SignatureState::Valid
    );
    let tampered = data
        .replace("Pay 10 EUR", "Pay 99 EUR")
        .replace("Message-ID: <", "Message-ID: <tampered-");
    let bad = deliver_home(&f, &tampered).await;
    let shown = body(&f, &bad).await;
    assert_eq!(
        shown.security.unwrap().signatures[0].state,
        SignatureState::Invalid
    );
    f.service.shutdown().await;
}

#[tokio::test]
async fn smime_round_trip() {
    let f = setup().await;
    let crypto = SendCrypto {
        technology: Some(CryptoTechnology::Smime),
        ..signed_encrypted()
    };
    let sent = send(
        &f,
        draft(MULTI_HOME_EMAIL, "S/MIME hello", "Certified greetings."),
        Some(crypto),
    )
    .await;
    assert!(matches!(sent, Response::SendQueued { .. }), "{sent:?}");
    let mail = received_count(&f, 1).await;
    let data = &mail[0].1;
    assert!(data.contains("application/pkcs7-mime"), "{data}");
    assert!(data.contains("Subject: S/MIME hello"), "{data}");
    assert!(!data.contains("Certified greetings"), "{data}");
    let message = deliver_home(&f, data).await;
    let shown = body(&f, &message).await;
    let security = shown.security.unwrap();
    assert_eq!(security.technology, CryptoTechnology::Smime);
    assert_eq!(
        security.encryption.unwrap().state,
        DecryptionState::Decrypted
    );
    assert_eq!(security.signatures[0].state, SignatureState::Valid);
    assert_eq!(
        security.signatures[0].chain.as_ref().unwrap().state,
        ChainState::Valid
    );
    assert!(shown.text.contains("Certified greetings"));
    f.service.shutdown().await;
}

#[tokio::test]
async fn bcc_recipients_get_their_own_encrypted_copy() {
    let f = setup().await;
    let mut d = draft(MULTI_HOME_EMAIL, "With Bcc", "Hello.");
    // The sender's own address as a Bcc recipient: it has a key.
    d.bcc = vec![Address {
        name: None,
        email: MULTI_WORK_EMAIL.into(),
    }];
    let crypto = SendCrypto {
        encrypt: true,
        ..SendCrypto::none()
    };
    let sent = send(&f, d, Some(crypto)).await;
    assert!(matches!(sent, Response::SendQueued { .. }), "{sent:?}");
    let mail = received_count(&f, 2).await;
    let envelopes: Vec<&Vec<String>> = mail.iter().map(|(to, _)| to).collect();
    assert!(envelopes.contains(&&vec![MULTI_HOME_EMAIL.to_owned()]));
    assert!(envelopes.contains(&&vec![MULTI_WORK_EMAIL.to_owned()]));
    for (_, data) in &mail {
        assert!(data.contains("multipart/encrypted"), "{data}");
        assert!(!data.contains("Bcc:"), "{data}");
    }
    f.service.shutdown().await;
}

#[tokio::test]
async fn missing_keys_and_locked_keys_refuse_to_send() {
    let f = setup().await;
    // Nobody has a key for grace@navy.example.
    let refused = send(
        &f,
        draft("grace@navy.example", "No key", "x"),
        Some(signed_encrypted()),
    )
    .await;
    assert!(
        matches!(&refused, Response::Error { code: ErrorCode::NoKey, message } if message.contains("grace@navy.example")),
        "{refused:?}"
    );
    // A passphrase key must be unlocked to sign.
    let Response::KeyGenerated { key } = request(
        &f,
        Request::GenerateKey {
            identity_id: MULTI_HOME_EMAIL.into(),
            algorithm: Default::default(),
            expires_days: None,
            protection: nail_api::security::SecretProtection::Passphrase {
                passphrase: "correct horse".into(),
            },
        },
    )
    .await
    else {
        panic!("key not generated")
    };
    assert!(key.has_secret);
    // Point the home identity at that key.
    let Response::AccountsConfig { config } = request(&f, Request::GetAccountsConfig).await else {
        panic!()
    };
    let mut account = config
        .accounts
        .into_iter()
        .find(|a| a.id.as_str() == MULTI_HOME_ID)
        .unwrap();
    account.identities[0]
        .crypto
        .get_or_insert_default()
        .openpgp_key = Some(key.fingerprint.clone());
    assert!(matches!(
        request(
            &f,
            Request::UpdateAccount {
                config: Box::new(account),
                secrets: vec![]
            }
        )
        .await,
        Response::AccountSaved { .. }
    ));
    let mut d = draft(MULTI_WORK_EMAIL, "Locked", "x");
    d.identity_id = MULTI_HOME_EMAIL.into();
    let sign = SendCrypto {
        sign: true,
        ..SendCrypto::none()
    };
    let refused = send(&f, d.clone(), Some(sign.clone())).await;
    assert!(
        matches!(
            &refused,
            Response::Error {
                code: ErrorCode::Locked,
                ..
            }
        ),
        "{refused:?}"
    );
    let wrong = request(
        &f,
        Request::UnlockKey {
            key_id: key.id.clone(),
            passphrase: "wrong".into(),
            remember_minutes: None,
        },
    )
    .await;
    assert!(
        matches!(
            &wrong,
            Response::Error {
                code: ErrorCode::AuthFailed,
                ..
            }
        ),
        "{wrong:?}"
    );
    assert_eq!(
        request(
            &f,
            Request::UnlockKey {
                key_id: key.id.clone(),
                passphrase: "correct horse".into(),
                remember_minutes: Some(5),
            },
        )
        .await,
        Response::Done
    );
    let sent = send(&f, d, Some(sign)).await;
    assert!(matches!(sent, Response::SendQueued { .. }), "{sent:?}");
    // Nothing went out in plain text.
    let mail = received_count(&f, 1).await;
    assert_eq!(mail.len(), 1);
    assert!(mail[0].1.contains("multipart/signed"));
    f.service.shutdown().await;
}

#[tokio::test]
async fn autocrypt_enables_encryption_after_one_message() {
    let f = setup().await;
    let check = || async {
        match request(
            &f,
            Request::CheckRecipientKeys {
                identity_id: MULTI_WORK_EMAIL.into(),
                recipients: vec!["alice@example.org".into()],
                lookup: false,
            },
        )
        .await
        {
            Response::RecipientKeys { report } => *report,
            other => panic!("unexpected {other:?}"),
        }
    };
    let before = check().await;
    assert_eq!(before.autocrypt, AutocryptRecommendation::Disable);
    assert!(!before.suggested.encrypt);
    assert!(before.recipients[0].openpgp.is_none());

    let raw =
        String::from_utf8(crypto_fixture(CryptoFixtures::Pgp, "autocrypt-alice.eml").unwrap())
            .unwrap();
    deliver_work(&f, &raw, "Hi with Autocrypt").await;
    let mut report = check().await;
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    while report.autocrypt != AutocryptRecommendation::Encrypt {
        assert!(tokio::time::Instant::now() < deadline, "{report:?}");
        tokio::time::sleep(Duration::from_millis(100)).await;
        report = check().await;
    }
    assert!(report.suggested.encrypt);
    let alice = &report.recipients[0];
    assert_eq!(alice.openpgp.as_ref().unwrap().source, KeySource::Autocrypt);
    assert!(alice.autocrypt.is_some());
    assert!(report.openpgp.own_key && report.openpgp.all_recipients);
    f.service.shutdown().await;
}

#[tokio::test]
async fn sender_authentication_and_link_warnings() {
    let f = setup().await;
    let raw = "Received: from mail.example.org by mx.nail.example; Tue, 29 Sep 2026 08:00:00 +0000\r\n\
Authentication-Results: mx.nail.example; dkim=pass header.d=example.org header.s=s1; spf=pass smtp.mailfrom=example.org; dmarc=pass header.from=example.org\r\n\
Authentication-Results: evil.example; dkim=pass header.d=bank.example\r\n\
From: Grace <grace@example.org>\r\nTo: ada@work.example\r\nSubject: Your account\r\n\
Date: Tue, 29 Sep 2026 08:00:00 +0000\r\nMessage-ID: <auth@example.org>\r\nMIME-Version: 1.0\r\n\
Content-Type: text/html; charset=utf-8\r\n\r\n\
<p>Please check <a href=\"https://xn--pple-43d.com/login\">your account</a>.</p>\r\n";
    let message = deliver_work(&f, raw, "Your account").await;
    let shown = body(&f, &message).await;
    let auth = shown.auth_results.expect("trusted results");
    assert_eq!(auth.authserv_id, "mx.nail.example");
    assert_eq!(auth.summary(), AuthVerdict::Pass);
    assert!(
        shown
            .warnings
            .iter()
            .any(|w| matches!(w, MessageWarning::LookalikeDomain { .. })),
        "{:?}",
        shown.warnings
    );
    // An unsigned plain message has no end-to-end status.
    assert!(shown.security.is_none());
    f.service.shutdown().await;
}

/// Answers every URL with `body`.
struct FixedHttp(Vec<u8>);

impl HttpClient for FixedHttp {
    fn get<'a>(&'a self, _url: &'a str) -> BoxFuture<'a, Result<HttpResponse, EngineError>> {
        let body = self.0.clone();
        Box::pin(async move { Ok(HttpResponse { status: 200, body }) })
    }
}

#[tokio::test]
async fn key_requests() {
    let data = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    std::fs::write(
        config.path().join("accounts.kdl"),
        "account \"me\" email=\"me@example.org\" {\n    imap host=\"127.0.0.1\" port=1 tls=\"plaintext\"\n    identity \"me@example.org\" name=\"Me\"\n}\n",
    )
    .unwrap();
    let alice = crypto_fixture(CryptoFixtures::Pgp, "alice.pub.asc").unwrap();
    let mut options =
        nail_core::demo::demo_options(ServiceOptions::new(data.path(), config.path()));
    options.watch_config = false;
    options.start_engines = false;
    options.key_lookup = Some(Arc::new(FixedHttp(alice.clone())));
    let service = Service::open(options).unwrap();
    let call = |r: Request| {
        let service = service.clone();
        async move { service.handle(r).await }
    };

    // Lookup: found, nothing imported.
    let Response::KeyLookup { candidates, .. } = call(Request::LookupKeys {
        email: "Alice@Example.org".into(),
        sources: vec![KeyLookupSource::KeysOpenpgpOrg],
    })
    .await
    else {
        panic!()
    };
    assert_eq!(candidates.len(), 1);
    assert!(candidates[0].known.is_none());
    let Response::Keys { keys } = call(Request::ListKeys {
        filter: KeyFilter::default(),
    })
    .await
    else {
        panic!()
    };
    assert!(keys.is_empty());

    // Import the found key (armored) and an S/MIME certificate (PEM).
    let Response::KeysImported { result } = call(Request::ImportKeys {
        data: candidates[0].armored.clone(),
        encoding: KeyEncoding::Text,
    })
    .await
    else {
        panic!()
    };
    assert_eq!(result.created, 1);
    let alice_id = result.keys[0].id.clone();
    let cert =
        String::from_utf8(crypto_fixture(CryptoFixtures::Smime, "bob.crt").unwrap()).unwrap();
    let Response::KeysImported { result } = call(Request::ImportKeys {
        data: cert,
        encoding: KeyEncoding::Text,
    })
    .await
    else {
        panic!()
    };
    assert_eq!(result.keys[0].technology, CryptoTechnology::Smime);
    let garbage = call(Request::ImportKeys {
        data: "hello".into(),
        encoding: KeyEncoding::Text,
    })
    .await;
    assert!(
        matches!(
            garbage,
            Response::Error {
                code: ErrorCode::Crypto,
                ..
            }
        ),
        "{garbage:?}"
    );

    // A generated own key lists first.
    let Response::KeyGenerated { key: own } = call(Request::GenerateKey {
        identity_id: "me@example.org".into(),
        algorithm: Default::default(),
        expires_days: Some(365),
        protection: Default::default(),
    })
    .await
    else {
        panic!()
    };
    assert_eq!(own.user_ids, ["Me <me@example.org>"]);
    let Response::Keys { keys } = call(Request::ListKeys {
        filter: KeyFilter::default(),
    })
    .await
    else {
        panic!()
    };
    assert_eq!(keys.len(), 3);
    assert!(keys[0].has_secret);

    // Trust, export, lookup marks it known, delete (own keys need the flag).
    let Response::KeySaved { key } = call(Request::SetKeyTrust {
        key_id: alice_id.clone(),
        decision: TrustDecision::Verified,
    })
    .await
    else {
        panic!()
    };
    assert_eq!(key.trust, TrustLevel::Verified);
    let Response::KeyExported { data, .. } = call(Request::ExportPublicKey {
        key_id: alice_id.clone(),
        encoding: KeyEncoding::Text,
    })
    .await
    else {
        panic!()
    };
    assert!(data.contains("BEGIN PGP PUBLIC KEY BLOCK"));
    let Response::KeyLookup { candidates, .. } = call(Request::LookupKeys {
        email: "alice@example.org".into(),
        sources: vec![],
    })
    .await
    else {
        panic!()
    };
    assert_eq!(candidates[0].known.as_ref(), Some(&alice_id));
    let refused = call(Request::DeleteKey {
        key_id: own.id.clone(),
        delete_secret: false,
    })
    .await;
    assert!(
        matches!(
            refused,
            Response::Error {
                code: ErrorCode::Conflict,
                ..
            }
        ),
        "{refused:?}"
    );
    assert_eq!(
        call(Request::DeleteKey {
            key_id: own.id.clone(),
            delete_secret: true,
        })
        .await,
        Response::Done
    );

    // Lookup off: refused.
    let Response::SecuritySettings { .. } = call(Request::UpdateSecuritySettings {
        settings: SecuritySettings {
            key_lookup: nail_api::security::KeyLookupPolicy::Off,
            ..SecuritySettings::default()
        },
    })
    .await
    else {
        panic!()
    };
    let off = call(Request::LookupKeys {
        email: "alice@example.org".into(),
        sources: vec![],
    })
    .await;
    assert!(
        matches!(
            off,
            Response::Error {
                code: ErrorCode::Unsupported,
                ..
            }
        ),
        "{off:?}"
    );
}

#[tokio::test]
async fn autocrypt_setup_message_is_sent_and_imported() {
    let f = setup().await;
    let Response::AutocryptSetupSent { passcode, .. } = request(
        &f,
        Request::SendAutocryptSetup {
            identity_id: MULTI_WORK_EMAIL.into(),
        },
    )
    .await
    else {
        panic!("setup message not sent")
    };
    assert_eq!(passcode.len(), 44);
    let mail = received_count(&f, 1).await;
    let (to, data) = &mail[0];
    assert_eq!(to, &[MULTI_WORK_EMAIL.to_owned()]);
    assert!(data.contains("Autocrypt-Setup-Message: v1"), "{data}");
    assert!(data.contains("Subject: Autocrypt Setup Message"), "{data}");

    // A setup message from another device (GnuPG-made, Bob's key).
    let raw =
        String::from_utf8(crypto_fixture(CryptoFixtures::Pgp, "autocrypt-setup-bob.eml").unwrap())
            .unwrap();
    let code = String::from_utf8(
        crypto_fixture(CryptoFixtures::Pgp, "autocrypt-setup-bob.passcode").unwrap(),
    )
    .unwrap();
    let subject = nail_mime::parse_message(raw.as_bytes()).headers.subject;
    let message = deliver_work(&f, &raw, &subject).await;
    let wrong = request(
        &f,
        Request::ImportAutocryptSetup {
            message_id: message.id.clone(),
            passcode: "1111-1111-1111-1111-1111-1111-1111-1111-1111".into(),
        },
    )
    .await;
    assert!(
        matches!(
            wrong,
            Response::Error {
                code: ErrorCode::AuthFailed,
                ..
            }
        ),
        "{wrong:?}"
    );
    let Response::KeysImported { result } = request(
        &f,
        Request::ImportAutocryptSetup {
            message_id: message.id,
            passcode: code.trim().into(),
        },
    )
    .await
    else {
        panic!("not imported")
    };
    assert!(result.keys[0].has_secret);
    assert_eq!(result.keys[0].source, KeySource::SetupMessage);
    f.service.shutdown().await;
}

async fn search_count(f: &Fixture, text: &str) -> u32 {
    match request(
        f,
        Request::Search {
            query: SearchQuery {
                text: text.into(),
                account_id: None,
                folder_id: None,
                offset: 0,
                limit: 50,
                sort: Default::default(),
            },
        },
    )
    .await
    {
        Response::SearchResults { total, .. } => total,
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn decrypted_content_is_indexed_only_after_opting_in() {
    let f = setup().await;
    send(
        &f,
        draft(MULTI_HOME_EMAIL, "Indexed secret", "zanzibarian marmalade"),
        Some(SendCrypto {
            encrypt: true,
            ..SendCrypto::none()
        }),
    )
    .await;
    let mail = received_count(&f, 1).await;
    let _first = deliver_home(&f, &mail[0].1).await;
    // Not opted in: the decrypted body is not searchable.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(search_count(&f, "zanzibarian").await, 0);

    // Opt in for the home account: its encrypted mail is reindexed.
    let Response::SecuritySettings { .. } = request(
        &f,
        Request::UpdateSecuritySettings {
            settings: SecuritySettings {
                index_decrypted: vec![AccountId::new(MULTI_HOME_ID)],
                ..SecuritySettings::default()
            },
        },
    )
    .await
    else {
        panic!()
    };
    eventually("the decrypted body is searchable", || async {
        search_count(&f, "zanzibarian").await > 0
    })
    .await;
    // Withdrawn again: gone from the index.
    request(
        &f,
        Request::UpdateSecuritySettings {
            settings: SecuritySettings::default(),
        },
    )
    .await;
    eventually("the decrypted body is gone", || async {
        search_count(&f, "zanzibarian").await == 0
    })
    .await;
    f.service.shutdown().await;
}

async fn vault_state(f: &Fixture) -> nail_api::security::VaultStatus {
    match request(f, Request::GetVaultStatus).await {
        Response::VaultStatus { status } => status,
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn encryption_at_rest_on_and_off_keeps_the_data() {
    let f = setup().await;
    let message = inbox(&f, MULTI_WORK_ID)
        .await
        .into_iter()
        .find(|m| m.subject.contains("Release notes"))
        .unwrap();
    let before = body(&f, &message).await;
    assert!(search_count(&f, "unified").await > 0);
    // Blob files start with the sealed-data magic only when encrypted.
    let blobs = f.data_dir.path().join("blobs");
    let is_sealed = |p: &std::path::PathBuf| {
        std::fs::read(p)
            .unwrap()
            .starts_with(&nail_store::SEALED_MAGIC)
    };
    let files = walk(&blobs);
    assert!(!files.is_empty());
    assert!(!files.iter().any(is_sealed));

    let Response::VaultStatus { status } =
        request(&f, Request::SetVaultEncryption { enabled: true }).await
    else {
        panic!()
    };
    assert_eq!(status.state, VaultState::Encrypting);
    assert!(status.key_in_keychain);
    eventually("encrypted", || async {
        vault_state(&f).await.state == VaultState::Encrypted
    })
    .await;
    assert_eq!(body(&f, &message).await, before);
    eventually("the index answers again", || async {
        search_count(&f, "unified").await > 0
    })
    .await;
    // Every blob on disk is sealed now.
    let files = walk(&blobs);
    assert!(!files.is_empty());
    assert!(files.iter().all(is_sealed), "{files:?}");

    request(&f, Request::SetVaultEncryption { enabled: false }).await;
    eventually("plain again", || async {
        vault_state(&f).await.state == VaultState::Plain
    })
    .await;
    assert!(!walk(&blobs).iter().any(is_sealed));
    assert_eq!(body(&f, &message).await, before);
    eventually("the index answers again", || async {
        search_count(&f, "unified").await > 0
    })
    .await;
    f.service.shutdown().await;
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(walk(&path));
        } else {
            out.push(path);
        }
    }
    out
}

#[tokio::test]
async fn mail_encrypted_to_someone_else_shows_no_key() {
    let f = setup().await;
    let raw =
        String::from_utf8(crypto_fixture(CryptoFixtures::Pgp, "encrypted-to-bob.eml").unwrap())
            .unwrap();
    assert!(nail_mcp::core_data::is_encrypted_mime(raw.as_bytes()));
    let subject = nail_mime::parse_message(raw.as_bytes()).headers.subject;
    let message = deliver_work(&f, &raw, &subject).await;
    let shown = body(&f, &message).await;
    // Not for us: no key.
    assert_eq!(
        shown.security.unwrap().encryption.unwrap().state,
        DecryptionState::NoKey
    );
    f.service.shutdown().await;
}
