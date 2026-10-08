//! Autocrypt Level 1: headers, peer state, gossip, recommendation and
//! setup messages (incl. GnuPG-made vectors).

use crate::common::{Env, fixture, fixture_text};
use nail_api::AccountId;
use nail_api::security::{
    AutocryptPreference, AutocryptRecommendation, DecryptionState, KeyAlgorithm, KeySource,
    RecipientKeyState, SecretProtection, SecretStorage, SignatureState, TrustDecision, TrustLevel,
};
use nail_pgp::autocrypt::{
    AutocryptHeader, IncomingMessage, SetupMessageText, create_setup_message, import_setup,
    is_setup_message, new_passcode, process_incoming, recipient_status, recommend,
};
use nail_pgp::compose::{Protection, protect_message, with_header};
use nail_pgp::{Error, Problem};
use pretty_assertions::assert_eq;

use nail_util::time::SECS_PER_DAY as DAY;
/// 2026-01-01 12:00 UTC, the Date of the fixtures.
const JAN_1: i64 = 1_767_268_800;

fn account() -> AccountId {
    AccountId("acc-1".into())
}

fn plain(from: &str, to: &str, date: &str, extra: &str) -> Vec<u8> {
    format!(
        "From: {from}\r\nTo: {to}\r\nSubject: hi\r\nDate: {date}\r\n{extra}Content-Type: text/plain\r\n\r\nhello\r\n"
    )
    .into_bytes()
}

fn alice_header_value() -> String {
    let incoming = IncomingMessage::from_raw(&fixture("autocrypt-alice.eml"));
    incoming.autocrypt[0].clone()
}

#[test]
fn autocrypt_header_from_a_gnupg_key_creates_a_peer() {
    let env = Env::new();
    let ring = env.keyring().at(JAN_1 + DAY);
    let incoming = IncomingMessage::from_raw(&fixture("autocrypt-alice.eml"));
    assert_eq!(incoming.from, vec!["alice@example.org"]);
    assert_eq!(incoming.recipients, vec!["bob@example.net"]);
    assert_eq!(incoming.date, Some(JAN_1));
    assert_eq!(incoming.autocrypt.len(), 1);

    let update = process_incoming(&ring, &account(), &incoming, &[]).unwrap();
    assert!(update.peer_updated);
    assert_eq!(update.keys.len(), 1);
    let peer = env
        .store
        .autocrypt_peer(&account(), "alice@example.org")
        .unwrap()
        .unwrap();
    assert_eq!(peer.last_seen, JAN_1);
    assert_eq!(peer.autocrypt_timestamp, Some(JAN_1));
    assert_eq!(peer.prefer_encrypt, AutocryptPreference::Mutual);
    let key = ring.get(&update.keys[0]).unwrap();
    assert_eq!(key.source, KeySource::Autocrypt);
    assert_eq!(key.trust, TrustLevel::Autocrypt);
    assert_eq!(
        peer.key_fingerprint.as_deref(),
        Some(key.fingerprint.as_str())
    );

    let status = recipient_status(&ring, &account(), "Alice@Example.org").unwrap();
    assert_eq!(status.email, "alice@example.org");
    assert_eq!(status.openpgp.unwrap().state, RecipientKeyState::Usable);
    let api_peer = status.autocrypt.unwrap();
    assert_eq!(
        api_peer.autocrypt_at.as_deref(),
        Some("2026-01-01T12:00:00Z")
    );
    assert_eq!(api_peer.prefer_encrypt, AutocryptPreference::Mutual);

    let recipients = vec!["alice@example.org".to_owned()];
    let mutual = recommend(
        &ring,
        &account(),
        "bob@example.net",
        AutocryptPreference::Mutual,
        true,
        &recipients,
        false,
    )
    .unwrap();
    assert_eq!(mutual, AutocryptRecommendation::Encrypt);
    let nopref = recommend(
        &ring,
        &account(),
        "bob@example.net",
        AutocryptPreference::NoPreference,
        true,
        &recipients,
        false,
    )
    .unwrap();
    assert_eq!(nopref, AutocryptRecommendation::Available);
    // Replies to encrypted mail are encrypted.
    let reply = recommend(
        &ring,
        &account(),
        "bob@example.net",
        AutocryptPreference::NoPreference,
        true,
        &recipients,
        true,
    )
    .unwrap();
    assert_eq!(reply, AutocryptRecommendation::Encrypt);
    // Without an own key nothing can be encrypted.
    let no_own = recommend(
        &ring,
        &account(),
        "bob@example.net",
        AutocryptPreference::Mutual,
        false,
        &recipients,
        false,
    )
    .unwrap();
    assert_eq!(no_own, AutocryptRecommendation::Disable);
}

#[test]
fn peer_state_follows_the_update_rules() {
    let env = Env::new();
    let ring = env.keyring().at(JAN_1 + 100 * DAY);
    let header = format!("Autocrypt: {}\r\n", alice_header_value());
    let alice = "Alice <alice@example.org>";
    let jan1 = "Thu, 01 Jan 2026 12:00:00 +0000";
    let msg = plain(alice, "bob@example.net", jan1, &header);
    process_incoming(&ring, &account(), &IncomingMessage::from_raw(&msg), &[]).unwrap();

    // An older message without the header changes nothing.
    let older = plain(
        alice,
        "bob@example.net",
        "Mon, 01 Dec 2025 12:00:00 +0000",
        "",
    );
    let update =
        process_incoming(&ring, &account(), &IncomingMessage::from_raw(&older), &[]).unwrap();
    assert!(!update.peer_updated);

    // A newer one without the header moves last_seen only.
    let later = plain(
        alice,
        "bob@example.net",
        "Sat, 21 Feb 2026 12:00:00 +0000",
        "",
    );
    process_incoming(&ring, &account(), &IncomingMessage::from_raw(&later), &[]).unwrap();
    let peer = env
        .store
        .autocrypt_peer(&account(), "alice@example.org")
        .unwrap()
        .unwrap();
    assert_eq!(peer.autocrypt_timestamp, Some(JAN_1));
    assert!(peer.last_seen > JAN_1 + 35 * DAY);
    // The Autocrypt key is now stale: discourage.
    let recipients = vec!["alice@example.org".to_owned()];
    let rec = recommend(
        &ring,
        &account(),
        "bob@example.net",
        AutocryptPreference::Mutual,
        true,
        &recipients,
        false,
    )
    .unwrap();
    assert_eq!(rec, AutocryptRecommendation::Discourage);

    // A future Date is clamped to now.
    let future = plain(
        alice,
        "bob@example.net",
        "Fri, 01 Jan 2100 00:00:00 +0000",
        &header,
    );
    process_incoming(&ring, &account(), &IncomingMessage::from_raw(&future), &[]).unwrap();
    let peer = env
        .store
        .autocrypt_peer(&account(), "alice@example.org")
        .unwrap()
        .unwrap();
    assert_eq!(peer.last_seen, JAN_1 + 100 * DAY);
    assert_eq!(peer.autocrypt_timestamp, Some(JAN_1 + 100 * DAY));
}

#[test]
fn invalid_senders_and_headers_are_ignored() {
    let env = Env::new();
    let ring = env.keyring().at(JAN_1 + DAY);
    let value = alice_header_value();
    let jan1 = "Thu, 01 Jan 2026 12:00:00 +0000";
    // Header for another address than From.
    let msg = plain(
        "mallory@example.org",
        "bob@example.net",
        jan1,
        &format!("Autocrypt: {value}\r\n"),
    );
    let update =
        process_incoming(&ring, &account(), &IncomingMessage::from_raw(&msg), &[]).unwrap();
    assert!(update.keys.is_empty(), "{:?}", update.keys);
    // Two headers for the sender: none counts.
    let two = format!("Autocrypt: {value}\r\nAutocrypt: {value}\r\n");
    let msg = plain("alice@example.org", "bob@example.net", jan1, &two);
    let update =
        process_incoming(&ring, &account(), &IncomingMessage::from_raw(&msg), &[]).unwrap();
    assert!(update.keys.is_empty(), "{:?}", update.keys);
    let peer = env
        .store
        .autocrypt_peer(&account(), "alice@example.org")
        .unwrap()
        .unwrap();
    assert_eq!(peer.autocrypt_timestamp, None);
    // Several From addresses, or a report: nothing at all.
    let msg = plain(
        "alice@example.org, carol@example.com",
        "bob@example.net",
        jan1,
        &format!("Autocrypt: {value}\r\n"),
    );
    let update =
        process_incoming(&ring, &account(), &IncomingMessage::from_raw(&msg), &[]).unwrap();
    assert_eq!(update, nail_pgp::autocrypt::AutocryptUpdate::default());
    let mut report =
        IncomingMessage::from_raw(&plain("carol@example.com", "bob@example.net", jan1, ""));
    report.is_report = true;
    let update = process_incoming(&ring, &account(), &report, &[]).unwrap();
    assert_eq!(update, nail_pgp::autocrypt::AutocryptUpdate::default());
    assert!(
        env.store
            .autocrypt_peer(&account(), "carol@example.com")
            .unwrap()
            .is_none()
    );
}

#[test]
fn gossip_about_recipients_discourages() {
    let env = Env::new();
    let ring = env.keyring().at(JAN_1 + DAY);
    let gossip = alice_header_value().replace("prefer-encrypt=mutual; ", "");
    let msg = plain(
        "carol@example.com",
        "bob@example.net, Alice <alice@example.org>",
        "Thu, 01 Jan 2026 12:00:00 +0000",
        "",
    );
    let incoming = IncomingMessage::from_raw(&msg);
    let update =
        process_incoming(&ring, &account(), &incoming, std::slice::from_ref(&gossip)).unwrap();
    assert_eq!(update.keys.len(), 1);
    let peer = env
        .store
        .autocrypt_peer(&account(), "alice@example.org")
        .unwrap()
        .unwrap();
    assert_eq!(peer.gossip_timestamp, Some(JAN_1));
    assert!(peer.gossip_fingerprint.is_some() && peer.key_fingerprint.is_none());
    assert_eq!(
        ring.get(&update.keys[0]).unwrap().trust,
        TrustLevel::AutocryptGossip
    );
    let rec = recommend(
        &ring,
        &account(),
        "bob@example.net",
        AutocryptPreference::Mutual,
        true,
        &["alice@example.org".to_owned()],
        false,
    )
    .unwrap();
    assert_eq!(rec, AutocryptRecommendation::Discourage);
    // A recipient without any key disables the whole message.
    let rec = recommend(
        &ring,
        &account(),
        "bob@example.net",
        AutocryptPreference::Mutual,
        true,
        &["alice@example.org".to_owned(), "dan@example.org".to_owned()],
        false,
    )
    .unwrap();
    assert_eq!(rec, AutocryptRecommendation::Disable);
    // Only the sender itself: nothing to encrypt to.
    let rec = recommend(
        &ring,
        &account(),
        "bob@example.net",
        AutocryptPreference::Mutual,
        true,
        &["BOB@example.net".to_owned()],
        false,
    )
    .unwrap();
    assert_eq!(rec, AutocryptRecommendation::Disable);

    // Gossip about someone who is not a recipient is ignored.
    let env = Env::new();
    let ring = env.keyring().at(JAN_1 + DAY);
    let msg = plain(
        "carol@example.com",
        "bob@example.net",
        "Thu, 01 Jan 2026 12:00:00 +0000",
        "",
    );
    let update = process_incoming(
        &ring,
        &account(),
        &IncomingMessage::from_raw(&msg),
        &[gossip],
    )
    .unwrap();
    assert!(update.keys.is_empty(), "{:?}", update.keys);
}

#[test]
fn rejected_autocrypt_key_disables_encryption() {
    let env = Env::new();
    let ring = env.keyring().at(JAN_1 + DAY);
    let update = process_incoming(
        &ring,
        &account(),
        &IncomingMessage::from_raw(&fixture("autocrypt-alice.eml")),
        &[],
    )
    .unwrap();
    ring.set_trust(&update.keys[0], TrustDecision::Rejected)
        .unwrap();
    let rec = recommend(
        &ring,
        &account(),
        "bob@example.net",
        AutocryptPreference::Mutual,
        true,
        &["alice@example.org".to_owned()],
        false,
    )
    .unwrap();
    assert_eq!(rec, AutocryptRecommendation::Disable);
}

/// Autocrypt enables encryption after one exchange: Ada's plain mail
/// carries her header; Bob can then encrypt to her, and she reads it.
#[test]
fn one_exchange_enables_encryption() {
    let ada = Env::new();
    let bob = Env::new();
    let ada_key = ada
        .keyring()
        .generate(
            "Ada",
            "ada@example.org",
            KeyAlgorithm::Curve25519,
            None,
            &SecretProtection::Keychain,
        )
        .unwrap();
    let bob_key = bob
        .keyring()
        .generate(
            "Bob",
            "bob@example.net",
            KeyAlgorithm::Curve25519,
            None,
            &SecretProtection::Keychain,
        )
        .unwrap();
    let now = chrono::Utc::now().timestamp();

    // Ada writes to Bob with her Autocrypt header.
    let ada_public = ada
        .keyring()
        .public_key(&ada.store.get_key(&ada_key.id).unwrap())
        .unwrap();
    let header = AutocryptHeader::for_key(
        &ada_public,
        "ada@example.org",
        AutocryptPreference::Mutual,
        now,
    )
    .unwrap();
    let date = chrono::Utc::now().to_rfc2822();
    let mail = with_header(
        &plain("Ada <ada@example.org>", "bob@example.net", &date, ""),
        "Autocrypt",
        &header.to_value(false),
    );
    let incoming = IncomingMessage::from_raw(&mail);
    assert_eq!(incoming.autocrypt.len(), 1);
    process_incoming(&bob.keyring(), &account(), &incoming, &[]).unwrap();
    let rec = recommend(
        &bob.keyring(),
        &account(),
        "bob@example.net",
        AutocryptPreference::Mutual,
        true,
        &["ada@example.org".to_owned()],
        false,
    )
    .unwrap();
    assert_eq!(rec, AutocryptRecommendation::Encrypt);

    // Bob replies encrypted (and signed); Ada decrypts it.
    let ring = bob.keyring();
    let recipients = ring
        .encryption_keys(&["ada@example.org".to_owned()])
        .unwrap();
    let signer = ring
        .unlocked_key(&bob.store.get_key(&bob_key.id).unwrap())
        .unwrap();
    let reply = protect_message(
        &plain("Bob <bob@example.net>", "ada@example.org", &date, ""),
        &Protection {
            signer: Some(&signer),
            recipients: &recipients,
            protect_headers: true,
            gossip: &[],
            now,
        },
    )
    .unwrap();
    let status = ada.status(&reply);
    assert_eq!(status.encryption.unwrap().state, DecryptionState::Decrypted);
    // Ada does not know Bob's key yet: the signer is unknown.
    assert_eq!(status.signatures[0].state, SignatureState::UnknownKey);
}

#[test]
fn gnupg_setup_message_imports_bobs_key() {
    let raw = fixture("autocrypt-setup-bob.eml");
    assert!(is_setup_message(&raw));
    let passcode = fixture_text("autocrypt-setup-bob.passcode");
    let env = Env::new();
    let ring = env.keyring();
    assert!(matches!(
        import_setup(&ring, &raw, "1111-1111-1111-1111-1111-1111-1111-1111-1111"),
        Err(Error::BadPassphrase)
    ));
    assert!(matches!(
        import_setup(&ring, &raw, "12"),
        Err(Error::BadPassphrase)
    ));
    let (result, prefer) = import_setup(&ring, &raw, passcode.trim()).unwrap();
    assert_eq!(prefer, AutocryptPreference::Mutual);
    let key = &result.keys[0];
    assert_eq!(key.emails, vec!["bob@example.net"]);
    assert_eq!(key.source, KeySource::SetupMessage);
    assert_eq!(key.secret_storage, Some(SecretStorage::Keychain));
    // It decrypts what GnuPG encrypted to bob.
    let status = env.status(&fixture("encrypted-to-bob.eml"));
    assert_eq!(status.encryption.unwrap().state, DecryptionState::Decrypted);

    // Not a setup message.
    assert!(matches!(
        import_setup(&ring, &fixture("signed-alice.eml"), passcode.trim()),
        Err(Error::Problem(Problem::NotSetupMessage))
    ));
}

#[test]
fn setup_message_roundtrip_between_devices() {
    let first = Env::new();
    let key = first
        .keyring()
        .generate(
            "Ada",
            "ada@example.org",
            KeyAlgorithm::Curve25519,
            None,
            &SecretProtection::Passphrase {
                passphrase: "pw".into(),
            },
        )
        .unwrap();
    first.keyring().unlock(&key.id, "pw", None).unwrap();
    let unlocked = first
        .keyring()
        .unlocked_key(&first.store.get_key(&key.id).unwrap())
        .unwrap();
    let passcode = new_passcode();
    let text = SetupMessageText {
        body: "Setup text".into(),
        html_intro: "Intro <b>".into(),
    };
    let raw = create_setup_message(
        &unlocked,
        "ada@example.org",
        AutocryptPreference::Mutual,
        &passcode,
        &text,
    )
    .unwrap();
    assert!(is_setup_message(&raw));
    let wire = String::from_utf8_lossy(&raw);
    assert!(wire.contains("Passphrase-Format: numeric9x4"));
    assert!(wire.contains(&format!("Passphrase-Begin: {}", &passcode[..2])));
    assert!(wire.contains("Intro &lt;b&gt;"));
    assert!(wire.contains("application/autocrypt-setup"));
    assert!(!wire.contains("PRIVATE KEY"), "the key is encrypted");

    let second = Env::new();
    let (result, prefer) =
        import_setup(&second.keyring(), &raw, &passcode.replace('-', " ")).unwrap();
    assert_eq!(prefer, AutocryptPreference::Mutual);
    assert_eq!(result.keys[0].fingerprint, key.fingerprint);
    assert_eq!(result.created, 1);
    // The second device signs with it without a passphrase (keychain).
    let stored = second.store.get_key(&result.keys[0].id).unwrap();
    assert!(second.keyring().unlocked_key(&stored).is_ok());
    assert!(matches!(
        create_setup_message(
            &unlocked,
            "ada@example.org",
            AutocryptPreference::Mutual,
            "123",
            &text
        ),
        Err(Error::Problem(Problem::SetupCodeFormat))
    ));
}

/// A fresh key (Ed25519 + X25519 encryption subkey, no passphrase) with
/// these user ids, the first primary.
fn key_with_user_ids(user_ids: &[&str]) -> pgp::composed::SignedSecretKey {
    use pgp::composed::{EncryptionCaps, KeyType, SecretKeyParamsBuilder, SubkeyParamsBuilder};
    use pgp::crypto::ecc_curve::ECCCurve;
    let subkey = SubkeyParamsBuilder::default()
        .key_type(KeyType::ECDH(ECCCurve::Curve25519Legacy))
        .can_encrypt(EncryptionCaps::All)
        .build()
        .unwrap();
    let mut params = SecretKeyParamsBuilder::default();
    params
        .key_type(KeyType::Ed25519Legacy)
        .can_certify(true)
        .can_sign(true)
        .primary_user_id(user_ids[0].to_owned())
        .subkeys(vec![subkey]);
    for extra in &user_ids[1..] {
        params.user_id(*extra);
    }
    params
        .build()
        .unwrap()
        .generate(rand::thread_rng())
        .unwrap()
}

/// The `Autocrypt` header value for `addr` carrying `key` as it is.
fn header_for(addr: &str, key: &pgp::composed::SignedSecretKey) -> String {
    AutocryptHeader {
        addr: addr.to_owned(),
        prefer_encrypt: AutocryptPreference::Mutual,
        keydata: nail_pgp::key::to_binary(&key.to_public_key()).unwrap(),
    }
    .to_value(false)
}

/// A PGP/MIME message from `from`, signed with `key`'s primary key.
fn signed_by(key: &pgp::composed::SignedSecretKey, from: &str) -> Vec<u8> {
    let inner = "Content-Type: text/plain; charset=us-ascii\r\n\r\nPay the invoice.";
    crate::common::signed_part_with(key, &format!("From: {from}\r\n"), inner)
}

fn today() -> String {
    chrono::Utc::now().to_rfc2822()
}

#[test]
fn autocrypt_keys_vouch_only_for_the_header_address() {
    let env = Env::new();
    let ring = env.keyring();
    let mallory = key_with_user_ids(&["CEO <ceo@bank.example>", "<mallory@evil.example>"]);
    let msg = plain(
        "mallory@evil.example",
        "bob@example.net",
        &today(),
        &format!(
            "Autocrypt: {}\r\n",
            header_for("mallory@evil.example", &mallory)
        ),
    );
    let update =
        process_incoming(&ring, &account(), &IncomingMessage::from_raw(&msg), &[]).unwrap();
    assert_eq!(update.keys.len(), 1);
    // Only the header's address is learned.
    let stored = ring.get(&update.keys[0]).unwrap();
    assert_eq!(stored.emails, vec!["mallory@evil.example"]);
    assert!(
        recipient_status(&ring, &account(), "ceo@bank.example")
            .unwrap()
            .openpgp
            .is_none()
    );
    let own = recipient_status(&ring, &account(), "mallory@evil.example")
        .unwrap()
        .openpgp
        .unwrap();
    assert_eq!(
        (own.state, own.trust),
        (RecipientKeyState::Usable, TrustLevel::Autocrypt)
    );
    // A mail "from the CEO" signed with that key does not match its From.
    let status = env.status(&signed_by(&mallory, "CEO <ceo@bank.example>"));
    let sig = &status.signatures[0];
    assert_eq!(sig.state, SignatureState::Valid);
    assert!(!sig.signer_matches_from, "{sig:?}");
    assert_eq!(sig.signer.as_deref(), Some("mallory@evil.example"));

    // A key without a user id for the header's address is not learned.
    let env = Env::new();
    let ring = env.keyring();
    let ceo_only = key_with_user_ids(&["CEO <ceo@bank.example>"]);
    let msg = plain(
        "mallory@evil.example",
        "bob@example.net",
        &today(),
        &format!(
            "Autocrypt: {}\r\n",
            header_for("mallory@evil.example", &ceo_only)
        ),
    );
    let update =
        process_incoming(&ring, &account(), &IncomingMessage::from_raw(&msg), &[]).unwrap();
    assert!(update.keys.is_empty(), "{:?}", update.keys);
    assert_eq!(
        ring.list(&nail_api::security::KeyFilter::default())
            .unwrap()
            .len(),
        0
    );

    // Gossip is bound to its address the same way.
    let gossip = header_for(
        "bob@example.net",
        &key_with_user_ids(&["<bob@example.net>", "CEO <ceo@bank.example>"]),
    )
    .replace("prefer-encrypt=mutual; ", "");
    let msg = plain(
        "carol@example.com",
        "bob@example.net, alice@example.org",
        &today(),
        "",
    );
    let update = process_incoming(
        &ring,
        &account(),
        &IncomingMessage::from_raw(&msg),
        &[gossip],
    )
    .unwrap();
    assert_eq!(update.keys.len(), 1);
    assert_eq!(
        ring.get(&update.keys[0]).unwrap().emails,
        vec!["bob@example.net"]
    );
    assert!(
        recipient_status(&ring, &account(), "ceo@bank.example")
            .unwrap()
            .openpgp
            .is_none()
    );
}

#[test]
fn autocrypt_trust_applies_per_address() {
    let env = Env::new();
    let ring = env.keyring();
    // The user imported a key that claims both addresses (e.g. from an
    // attachment); it has no trust.
    let mallory = key_with_user_ids(&["CEO <ceo@bank.example>", "<mallory@evil.example>"]);
    let imported = ring
        .import_bytes(
            &nail_pgp::key::to_binary(&mallory.to_public_key()).unwrap(),
            KeySource::Imported,
        )
        .unwrap();
    assert_eq!(imported.keys[0].emails.len(), 2);
    // An Autocrypt header for mallory's own address raises the key's
    // source to autocrypt ...
    let msg = plain(
        "mallory@evil.example",
        "bob@example.net",
        &today(),
        &format!(
            "Autocrypt: {}\r\n",
            header_for("mallory@evil.example", &mallory)
        ),
    );
    process_incoming(&ring, &account(), &IncomingMessage::from_raw(&msg), &[]).unwrap();
    let key = ring.get(&imported.keys[0].id).unwrap();
    assert_eq!(key.source, KeySource::Autocrypt);
    // ... but the trust counts only for that address.
    let for_mallory = ring
        .recipient_key("mallory@evil.example")
        .unwrap()
        .unwrap()
        .0;
    assert_eq!(for_mallory.trust, TrustLevel::Autocrypt);
    let for_ceo = ring.recipient_key("CEO@bank.example").unwrap().unwrap().0;
    assert_eq!(for_ceo.trust, TrustLevel::Unknown);
    // A signature "from the CEO" with mallory's key matches the From (the
    // key does carry the address) but is not trusted for it.
    let status = env.status(&signed_by(&mallory, "CEO <ceo@bank.example>"));
    let sig = &status.signatures[0];
    assert!(sig.signer_matches_from);
    assert_eq!(sig.trust, TrustLevel::Unknown);
    // For mallory's own address it is.
    let status = env.status(&signed_by(&mallory, "<mallory@evil.example>"));
    assert_eq!(status.signatures[0].trust, TrustLevel::Autocrypt);
    // A user decision applies to every address.
    ring.set_trust(&key.id, TrustDecision::Verified).unwrap();
    let status = env.status(&signed_by(&mallory, "CEO <ceo@bank.example>"));
    assert_eq!(status.signatures[0].trust, TrustLevel::Verified);
}
