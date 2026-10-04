//! Smartcards over the soft token (no hardware): listing, signing and
//! decrypting on the card, the PIN flow (prompt, retry, cancel, lock),
//! the PIN cache, removed / moved tokens and module problems.
#![cfg(test)]

use std::sync::Arc;

use nail_api::smartcard::{
    PinPurpose, Pkcs11Module, SmartcardError, SmartcardKeyRef, SmartcardKeyUsage,
    SmartcardObjectKind, SmartcardSettings,
};
use nail_pkcs11::testing::{SoftKey, SoftToken};
use nail_pkcs11::{
    DecryptMechanism, FixedPin, HashKind, KeyResolver, Module, PublicKeyMaterial, SignMechanism,
    Smartcards, digest_info,
};
use nail_testkit::{CryptoFixtures, crypto_fixture};
use pretty_assertions::assert_eq;
use rsa::pkcs8::DecodePrivateKey;

const MODULE: &str = r"C:\Program Files\Test\soft-pkcs11.dll";
const PIN: &str = "123456";

fn fixture(name: &str) -> Vec<u8> {
    crypto_fixture(CryptoFixtures::Smime, name).unwrap()
}

fn alice_key() -> rsa::RsaPrivateKey {
    rsa::RsaPrivateKey::from_pkcs8_pem(std::str::from_utf8(&fixture("alice.key")).unwrap()).unwrap()
}

fn bob_key() -> p256::SecretKey {
    p256::SecretKey::from_pkcs8_pem(std::str::from_utf8(&fixture("bob.key")).unwrap()).unwrap()
}

fn pem_to_der(pem: &[u8]) -> Vec<u8> {
    let (_, der) = der::pem::decode_vec(pem).unwrap();
    der
}

/// A module with one card: Alice's RSA key with her certificate, Bob's
/// P-256 key without (only the public key object).
fn token() -> Arc<SoftToken> {
    let token = Arc::new(SoftToken::new(MODULE));
    token.add_token(1, "Test card", "0006-0001", PIN);
    token.add_key(
        1,
        &[0x01],
        "Alice signing key",
        SoftKey::Rsa(Box::new(alice_key())),
        Some(pem_to_der(&fixture("alice.crt"))),
        false,
    );
    token.add_key(1, &[0x02], "Bob key", SoftKey::P256(bob_key()), None, true);
    token
}

fn cards(token: &Arc<SoftToken>, cache_minutes: u32, pins: Arc<FixedPin>) -> Smartcards {
    let module: Arc<dyn Module> = token.clone();
    Smartcards::from_modules(vec![module], cache_minutes, pins)
}

fn key_ref(id: &str) -> SmartcardKeyRef {
    SmartcardKeyRef {
        module: MODULE.into(),
        token_serial: "0006-0001".into(),
        object_id: id.into(),
    }
}

fn sha256(data: &[u8]) -> Vec<u8> {
    use sha2_legacy::Digest;
    sha2_legacy::Sha256::digest(data).to_vec()
}

#[test]
fn tokens_list_keys_certificates_and_usages() {
    let token = token();
    let cards = cards(&token, 0, Arc::new(FixedPin::default()));
    let (tokens, problems) = cards.list();
    assert!(problems.is_empty());
    assert_eq!(tokens.len(), 1);
    let card = &tokens[0];
    assert_eq!(
        (
            card.module.as_str(),
            card.slot_id,
            card.label.as_str(),
            card.serial.as_str()
        ),
        (MODULE, 1, "Test card", "0006-0001")
    );
    assert!(card.login_required && !card.pin_locked);
    let alice = card
        .objects
        .iter()
        .find(|o| o.kind == SmartcardObjectKind::PrivateKey && o.id == "01")
        .unwrap();
    // The private key hides its modulus; the algorithm comes from the
    // public key object with the same CKA_ID.
    assert_eq!(alice.algorithm.as_deref(), Some("RSA 2048"));
    assert_eq!(
        alice.usages,
        [SmartcardKeyUsage::Sign, SmartcardKeyUsage::Decrypt]
    );
    let cert = card
        .objects
        .iter()
        .find(|o| o.kind == SmartcardObjectKind::Certificate)
        .and_then(|o| o.certificate.clone())
        .unwrap();
    assert!(cert.subject.contains("Alice"), "{}", cert.subject);
    assert_eq!(cert.emails, ["alice@example.org"]);
    assert_eq!(cert.sha256.len(), 64);
    assert!(cert.not_before.starts_with("2020-01-01"));
    let bob = card.objects.iter().find(|o| o.id == "02").unwrap();
    assert_eq!(bob.algorithm.as_deref(), Some("ECC P-256"));
    // Listing never logs in.
    assert_eq!(token.stats().logins, 0);
}

#[test]
fn rsa_signing_asks_for_the_pin_once_and_verifies() {
    use rsa::traits::PublicKeyParts;
    let token = token();
    let pins = Arc::new(FixedPin::answers([Some(PIN)]));
    let cards = cards(&token, 10, Arc::clone(&pins));
    let key = cards.card_key(&key_ref("01")).unwrap();
    assert_eq!(key.label(), "Alice signing key");
    assert_eq!(key.token_label(), "Test card");
    assert!(key.certificate().is_some());
    let Some(PublicKeyMaterial::Rsa { modulus, .. }) = key.public_key() else {
        panic!("RSA public key from the paired object");
    };
    assert_eq!(*modulus, alice_key().n().to_bytes_be());

    let digest = sha256(b"hello card");
    let input = digest_info(HashKind::Sha256, &digest).unwrap();
    let signature = key.sign(SignMechanism::RsaPkcs1, &input).unwrap();
    rsa::RsaPublicKey::from(&alice_key())
        .verify(
            rsa::Pkcs1v15Sign::new::<sha2_legacy::Sha256>(),
            &digest,
            &signature,
        )
        .unwrap();
    let requests = pins.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].purpose, PinPurpose::Sign);
    assert_eq!(requests[0].token_serial, "0006-0001");
    assert_eq!(requests[0].key_label.as_deref(), Some("Alice signing key"));
    assert!(!requests[0].retry);

    // Within the PIN cache: no second prompt, no second login.
    key.sign(SignMechanism::RsaPkcs1, &input).unwrap();
    assert_eq!(pins.requests().len(), 1);
    assert_eq!(token.stats().logins, 1);
    // Logging out everything asks again next time.
    cards.logout_all();
    assert_eq!(token.stats().logouts, 1);
    assert!(matches!(
        key.sign(SignMechanism::RsaPkcs1, &input),
        Err(e) if e.kind == SmartcardError::PinCancelled
    ));
}

#[test]
fn without_a_pin_cache_every_operation_logs_out_again() {
    use p256::ecdsa::signature::hazmat::PrehashVerifier;

    let token = token();
    let pins = Arc::new(FixedPin::answers([Some(PIN), Some(PIN)]));
    let cards = cards(&token, 0, Arc::clone(&pins));
    let key = cards.card_key(&key_ref("02")).unwrap();
    let digest = sha256(b"ecdsa");
    let raw = key.sign(SignMechanism::Ecdsa, &digest).unwrap();
    assert_eq!(raw.len(), 64);
    let sig = p256::ecdsa::Signature::from_slice(&raw).unwrap();
    p256::ecdsa::VerifyingKey::from(bob_key().public_key())
        .verify_prehash(&digest, &sig)
        .unwrap();
    // The DER form for CMS parses as the same signature.
    let der = nail_pkcs11::ecdsa_raw_to_der(&raw).unwrap();
    assert_eq!(p256::ecdsa::Signature::from_der(&der).unwrap(), sig);
    key.sign(SignMechanism::Ecdsa, &digest).unwrap();
    assert_eq!(pins.requests().len(), 2);
    let stats = token.stats();
    assert_eq!((stats.logins, stats.logouts, stats.operations), (2, 2, 2));
}

#[test]
fn wrong_pins_retry_count_down_and_lock() {
    let token = token();
    let pins = Arc::new(FixedPin::answers([Some("000000"), Some(PIN)]));
    let cards = cards(&token, 5, Arc::clone(&pins));
    let key = cards.card_key(&key_ref("01")).unwrap();
    let input = digest_info(HashKind::Sha256, &sha256(b"x")).unwrap();
    key.sign(SignMechanism::RsaPkcs1, &input).unwrap();
    let requests = pins.requests();
    assert_eq!(requests.len(), 2);
    assert!(!requests[0].retry && requests[1].retry);

    // Three wrong PINs lock the card; the third prompt says one try is left.
    let token = self::token();
    let pins = Arc::new(FixedPin::answers([
        Some("1"),
        Some("2"),
        Some("3"),
        Some(PIN),
    ]));
    let cards = self::cards(&token, 5, Arc::clone(&pins));
    let key = cards.card_key(&key_ref("01")).unwrap();
    let err = key.sign(SignMechanism::RsaPkcs1, &input).unwrap_err();
    assert_eq!(err.kind, SmartcardError::PinLocked);
    let requests = pins.requests();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[2].attempts_left, Some(1));
    assert!(cards.list().0[0].pin_locked);
    // Locked: no prompt at all any more.
    let err = key.sign(SignMechanism::RsaPkcs1, &input).unwrap_err();
    assert_eq!(err.kind, SmartcardError::PinLocked);
    assert_eq!(pins.requests().len(), 3);
}

#[test]
fn cancelling_the_prompt_stops_the_operation() {
    let token = token();
    let pins = Arc::new(FixedPin::answers([None]));
    let cards = cards(&token, 5, pins);
    let key = cards.card_key(&key_ref("01")).unwrap();
    let err = key
        .decrypt(DecryptMechanism::RsaPkcs1, &[0; 256])
        .unwrap_err();
    assert_eq!(err.kind, SmartcardError::PinCancelled);
    let api: nail_api::ApiError = err.into();
    assert_eq!(api.code, nail_api::ErrorCode::Cancelled);
    assert_eq!(token.stats().operations, 0);
}

#[test]
fn decryption_and_ecdh_happen_on_the_card() {
    use p256::elliptic_curve::sec1::ToEncodedPoint;

    let token = token();
    let pins = Arc::new(FixedPin::answers([Some(PIN)]));
    let cards = cards(&token, 5, Arc::clone(&pins));
    let alice = cards.card_key(&key_ref("01")).unwrap();
    let public = rsa::RsaPublicKey::from(&alice_key());
    let mut rng = rand::rngs::OsRng;
    let ct = public
        .encrypt(&mut rng, rsa::Pkcs1v15Encrypt, b"content key")
        .unwrap();
    assert_eq!(
        alice
            .decrypt(DecryptMechanism::RsaPkcs1, &ct)
            .unwrap()
            .as_slice(),
        b"content key"
    );
    let ct = public
        .encrypt(
            &mut rng,
            rsa::Oaep::new::<sha2_legacy::Sha256>(),
            b"oaep key",
        )
        .unwrap();
    assert_eq!(
        alice
            .decrypt(DecryptMechanism::RsaOaep(HashKind::Sha256), &ct)
            .unwrap()
            .as_slice(),
        b"oaep key"
    );
    // A PIN prompt has a decrypt purpose.
    assert_eq!(pins.requests()[0].purpose, PinPurpose::Decrypt);

    let bob = cards.card_key(&key_ref("02")).unwrap();
    let peer = p256::SecretKey::random(&mut rng);
    let peer_point = peer.public_key().to_encoded_point(false);
    let on_card = bob.ecdh(peer_point.as_bytes()).unwrap();
    let expected =
        p256::ecdh::diffie_hellman(peer.to_nonzero_scalar(), bob_key().public_key().as_affine());
    assert_eq!(on_card.as_slice(), expected.raw_secret_bytes().as_slice());
    // Wrong mechanism for the key type: unsupported.
    let err = bob.decrypt(DecryptMechanism::RsaPkcs1, &[1]).unwrap_err();
    assert_eq!(err.kind, SmartcardError::MechanismUnsupported);
}

#[test]
fn removed_moved_and_unknown_tokens_and_keys() {
    let token = token();
    let pins = Arc::new(FixedPin::answers([Some(PIN), Some(PIN)]));
    let cards = cards(&token, 5, pins);
    let key = cards.card_key(&key_ref("01")).unwrap();
    let input = digest_info(HashKind::Sha256, &sha256(b"x")).unwrap();
    token.set_present(1, false);
    assert_eq!(
        key.sign(SignMechanism::RsaPkcs1, &input).unwrap_err().kind,
        SmartcardError::TokenNotPresent
    );
    assert!(cards.list().0.is_empty());
    // Inserted into another reader: found by its serial.
    token.set_present(1, true);
    token.move_token(1, 7);
    key.sign(SignMechanism::RsaPkcs1, &input).unwrap();

    assert_eq!(
        cards.card_key(&key_ref("99")).unwrap_err().kind,
        SmartcardError::KeyNotFound
    );
    assert_eq!(
        cards.card_key(&key_ref("zz")).unwrap_err().kind,
        SmartcardError::KeyNotFound
    );
    let mut other = key_ref("01");
    other.token_serial = "nope".into();
    assert_eq!(
        cards.card_key(&other).unwrap_err().kind,
        SmartcardError::TokenNotPresent
    );
    other = key_ref("01");
    other.module = r"C:\other.dll".into();
    assert_eq!(
        cards.card_key(&other).unwrap_err().kind,
        SmartcardError::ModuleNotFound
    );
    let described = cards.describe_key(&key_ref("01")).unwrap();
    assert!(described.certificate.is_some() && described.public_key.is_some());
}

#[test]
fn modules_that_do_not_load_are_reported() {
    let dir = tempfile::tempdir().unwrap();
    let fake = dir.path().join("broken.dll");
    std::fs::write(&fake, b"not a dll").unwrap();
    let settings = SmartcardSettings {
        modules: vec![
            Pkcs11Module {
                path: r"C:\missing\pkcs11.dll".into(),
                name: None,
                enabled: true,
            },
            Pkcs11Module {
                path: fake.to_string_lossy().into_owned(),
                name: Some("Broken".into()),
                enabled: true,
            },
            Pkcs11Module {
                path: r"C:\disabled\pkcs11.dll".into(),
                name: None,
                enabled: false,
            },
        ],
        pin_cache_minutes: 0,
    };
    let cards = Smartcards::load(&settings, Arc::new(FixedPin::default()));
    let kinds: Vec<_> = cards.problems().iter().map(|p| p.error).collect();
    assert_eq!(
        kinds,
        [
            SmartcardError::ModuleNotFound,
            SmartcardError::ModuleLoadFailed
        ]
    );
    let (tokens, problems) = cards.list();
    assert!(tokens.is_empty());
    assert_eq!(problems.len(), 2);
}
