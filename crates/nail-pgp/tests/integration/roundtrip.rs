//! PGP/MIME produced by Nail, read back by Nail: sign, encrypt, sign +
//! encrypt, header protection, and the failure cases (tampering, wrong
//! key, expired and revoked signers, header mismatch).

use crate::common::{Env, NOW, fixture, subject_and_text};
use nail_api::KeyId;
use nail_api::security::{
    DecryptionState, HeaderProtection, KeyAlgorithm, KeyEncoding, SecretProtection, SignatureState,
    TrustLevel,
};
use nail_pgp::Error;
use nail_pgp::compose::{OBSCURED_SUBJECT, Protection, protect_message};
use nail_pgp::key::{ParsedKey, parse_keys};
use nail_pgp::keyring::UnlockedKey;
use pgp::composed::{ArmorOptions, DetachedSignature, SignedPublicKey};
use pgp::crypto::hash::HashAlgorithm;
use pgp::packet::{SignatureConfig, SignatureType, Subpacket, SubpacketData};
use pgp::types::{KeyDetails, Password};
use pretty_assertions::assert_eq;

const PLAIN: &[u8] = b"From: Ada Lovelace <ada@example.org>\n\
To: Bob Example <bob@example.net>\n\
Subject: Engine notes\n\
Date: Tue, 29 Sep 2026 10:00:00 +0000\n\
Message-ID: <notes@example.org>\n\
MIME-Version: 1.0\n\
Content-Type: multipart/alternative; boundary=\"alt\"\n\
\n\
--alt\n\
Content-Type: text/plain; charset=utf-8\n\
Content-Transfer-Encoding: quoted-printable\n\
\n\
The analytical engine weaves algebraic patterns.=20\n\
\n\
--alt\n\
Content-Type: text/html; charset=utf-8\n\
\n\
<p>The analytical engine weaves algebraic patterns.</p>\n\
--alt--\n";

/// Ada with an own key, and Bob with an own key; each knows the other's
/// public key.
struct Pair {
    ada: Env,
    bob: Env,
    ada_key: KeyId,
    bob_key: KeyId,
}

fn generate(env: &Env, name: &str, email: &str) -> KeyId {
    env.keyring()
        .generate(
            name,
            email,
            KeyAlgorithm::Curve25519,
            None,
            &SecretProtection::Keychain,
        )
        .unwrap()
        .id
}

fn exchange(from: &Env, key: &KeyId, to: &Env) {
    let exported = from
        .keyring()
        .export_public(key, KeyEncoding::Text)
        .unwrap();
    to.keyring()
        .import(&exported.data, exported.encoding)
        .unwrap();
}

fn pair() -> Pair {
    let ada = Env::new();
    let bob = Env::new();
    let ada_key = generate(&ada, "Ada Lovelace", "ada@example.org");
    let bob_key = generate(&bob, "Bob Example", "bob@example.net");
    exchange(&ada, &ada_key, &bob);
    exchange(&bob, &bob_key, &ada);
    Pair {
        ada,
        bob,
        ada_key,
        bob_key,
    }
}

fn unlocked(env: &Env, key: &KeyId) -> UnlockedKey {
    let ring = env.keyring();
    ring.unlocked_key(&env.store.get_key(key).unwrap()).unwrap()
}

fn public(env: &Env, key: &KeyId) -> SignedPublicKey {
    env.keyring()
        .public_key(&env.store.get_key(key).unwrap())
        .unwrap()
}

/// The public key of `owner`'s key `key` as `holder` stores it (key ids
/// are per store; the fingerprint is the same everywhere).
fn peer_public(holder: &Env, owner: &Env, key: &KeyId) -> SignedPublicKey {
    let fingerprint = owner.keyring().get(key).unwrap().fingerprint;
    let stored = holder
        .store
        .find_key(nail_api::security::CryptoTechnology::OpenPgp, &fingerprint)
        .unwrap()
        .unwrap();
    holder.keyring().public_key(&stored).unwrap()
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn sign_only(env: &Env, key: &KeyId, protect_headers: bool) -> Vec<u8> {
    let signer = unlocked(env, key);
    protect_message(
        PLAIN,
        &Protection {
            signer: Some(&signer),
            recipients: &[],
            protect_headers,
            gossip: &[],
            now: now(),
        },
    )
    .unwrap()
}

#[test]
fn signed_message_verifies_and_keeps_its_content() {
    let p = pair();
    let signed = sign_only(&p.ada, &p.ada_key, false);
    let text = String::from_utf8_lossy(&signed);
    assert!(text.contains("multipart/signed; micalg=pgp-sha256"));
    assert!(text.contains("protocol=\"application/pgp-signature\""));
    assert!(text.contains("-----BEGIN PGP SIGNATURE-----"));
    assert!(
        !signed.windows(2).any(|w| w[1] == b'\n' && w[0] != b'\r'),
        "CRLF only"
    );
    // Outer headers stay; the subject is not hidden when only signing.
    assert_eq!(subject_and_text(&signed).0, "Engine notes");

    // Bob (knows Ada's key): valid, trust unknown.
    let outcome = p.bob.read(&signed);
    let status = outcome.status.unwrap();
    let sig = &status.signatures[0];
    assert_eq!(sig.state, SignatureState::Valid);
    assert!(sig.signer_matches_from);
    assert_eq!(sig.trust, TrustLevel::Unknown);
    let (subject, body) = subject_and_text(&outcome.display.unwrap());
    assert_eq!(subject, "Engine notes");
    assert!(body.contains("The analytical engine weaves algebraic patterns."));
    // Ada (own key): trust own.
    let status = p.ada.status(&signed);
    assert_eq!(status.signatures[0].trust, TrustLevel::Own);
}

#[test]
fn tampered_signed_body_is_invalid() {
    let p = pair();
    let signed = sign_only(&p.ada, &p.ada_key, false);
    let tampered = String::from_utf8(signed).unwrap().replacen(
        "algebraic patterns.=20",
        "algebraic pattern!=20",
        1,
    );
    let status = p.bob.status(tampered.as_bytes());
    assert_eq!(status.signatures[0].state, SignatureState::Invalid);
}

#[test]
fn signed_by_an_unknown_key() {
    let p = pair();
    let signed = sign_only(&p.ada, &p.ada_key, false);
    let stranger = Env::new();
    let status = stranger.status(&signed);
    assert_eq!(status.signatures[0].state, SignatureState::UnknownKey);
    let ada_fingerprint = p.ada.keyring().get(&p.ada_key).unwrap().fingerprint;
    assert_eq!(
        status.signatures[0].fingerprint.as_deref(),
        Some(ada_fingerprint.as_str())
    );
}

#[test]
fn signed_and_encrypted_with_header_protection() {
    let p = pair();
    let signer = unlocked(&p.ada, &p.ada_key);
    let recipients = vec![
        peer_public(&p.ada, &p.bob, &p.bob_key),
        public(&p.ada, &p.ada_key),
    ];
    let sent = protect_message(
        PLAIN,
        &Protection {
            signer: Some(&signer),
            recipients: &recipients,
            protect_headers: true,
            gossip: &["addr=carol@example.com; keydata=AAAA".to_owned()],
            now: now(),
        },
    )
    .unwrap();
    let wire = String::from_utf8_lossy(&sent);
    assert!(wire.contains("multipart/encrypted"));
    assert!(wire.contains("application/pgp-encrypted"));
    assert!(wire.contains("Version: 1"));
    assert!(!wire.contains("analytical"), "content must be encrypted");
    assert!(!wire.contains("Engine notes"), "the subject must be hidden");
    assert_eq!(subject_and_text(&sent).0, OBSCURED_SUBJECT);
    assert!(wire.contains("To: Bob Example <bob@example.net>"));

    let outcome = p.bob.read(&sent);
    let status = outcome.status.unwrap();
    let encryption = status.encryption.unwrap();
    assert_eq!(encryption.state, DecryptionState::Decrypted);
    assert_eq!(encryption.key_id.as_ref(), Some(&p.bob_key));
    assert_eq!(encryption.recipients.len(), 2);
    assert_eq!(status.signatures.len(), 1);
    assert_eq!(status.signatures[0].state, SignatureState::Valid);
    assert!(status.signatures[0].signer_matches_from);
    assert_eq!(status.header_protection, HeaderProtection::Protected);
    assert_eq!(
        outcome.gossip,
        vec!["addr=carol@example.com; keydata=AAAA".to_owned()]
    );
    let display = outcome.display.unwrap();
    let (subject, body) = subject_and_text(&display);
    assert_eq!(subject, "Engine notes");
    assert!(body.contains("The analytical engine"));
    assert!(!String::from_utf8_lossy(&display).contains("HP-Outer"));

    // The sender's copy is readable too.
    let status = p.ada.status(&sent);
    assert_eq!(status.encryption.unwrap().key_id.as_ref(), Some(&p.ada_key));
}

#[test]
fn encrypted_only_and_wrong_key() {
    let p = pair();
    let recipients = vec![peer_public(&p.ada, &p.bob, &p.bob_key)];
    let sent = protect_message(
        PLAIN,
        &Protection {
            signer: None,
            recipients: &recipients,
            protect_headers: false,
            gossip: &[],
            now: now(),
        },
    )
    .unwrap();
    // Without header protection the subject stays on the wire.
    assert_eq!(subject_and_text(&sent).0, "Engine notes");
    let status = p.bob.status(&sent);
    assert_eq!(status.encryption.unwrap().state, DecryptionState::Decrypted);
    assert!(status.signatures.is_empty());
    assert_eq!(status.header_protection, HeaderProtection::None);

    // Ada did not encrypt to herself: no key.
    let status = p.ada.status(&sent);
    assert_eq!(status.encryption.unwrap().state, DecryptionState::NoKey);
    // Someone with an unrelated own key: no key either.
    let carol = Env::new();
    generate(&carol, "Carol", "carol@example.com");
    let status = carol.status(&sent);
    assert_eq!(status.encryption.unwrap().state, DecryptionState::NoKey);
}

#[test]
fn tampered_ciphertext_fails() {
    let p = pair();
    let recipients = vec![peer_public(&p.ada, &p.bob, &p.bob_key)];
    let sent = protect_message(
        PLAIN,
        &Protection {
            signer: None,
            recipients: &recipients,
            protect_headers: true,
            gossip: &[],
            now: now(),
        },
    )
    .unwrap();
    let text = String::from_utf8(sent).unwrap();
    // Change one base64 character in the middle of the armored message.
    let start = text.find("-----BEGIN PGP MESSAGE-----").unwrap();
    let body_start = start + text[start..].find("\r\n\r\n").unwrap() + 4;
    let target = body_start + 120;
    let mut bytes = text.into_bytes();
    bytes[target] = if bytes[target] == b'A' { b'B' } else { b'A' };
    let outcome = p.bob.read(&bytes);
    let status = outcome.status.unwrap();
    assert_eq!(status.encryption.unwrap().state, DecryptionState::Failed);
    assert!(outcome.display.is_none());
}

/// A `multipart/signed` message over `PLAIN`'s content, signed directly
/// with rPGP (bypassing Nail's validity checks).
fn raw_signed_with(secret: &pgp::composed::SignedSecretKey) -> Vec<u8> {
    let inner = b"Content-Type: text/plain; charset=us-ascii\r\n\r\nSigned text.";
    let signature = DetachedSignature::sign_binary_data(
        rand::thread_rng(),
        &secret.primary_key,
        &Password::empty(),
        HashAlgorithm::Sha256,
        &inner[..],
    )
    .unwrap()
    .to_armored_string(ArmorOptions::default())
    .unwrap();
    let mut out = b"From: dave@example.org\r\nSubject: x\r\nMIME-Version: 1.0\r\n\
Content-Type: multipart/signed; micalg=pgp-sha256; protocol=\"application/pgp-signature\"; boundary=\"b\"\r\n\r\n--b\r\n"
        .to_vec();
    out.extend_from_slice(inner);
    out.extend_from_slice(b"\r\n--b\r\nContent-Type: application/pgp-signature\r\n\r\n");
    out.extend_from_slice(signature.replace('\n', "\r\n").as_bytes());
    out.extend_from_slice(b"\r\n--b--\r\n");
    out
}

/// A `multipart/signed` message with the outer header `outer` whose
/// signed part is `inner` (header fields + body), signed by `secret`.
fn signed_part_with(secret: &pgp::composed::SignedSecretKey, outer: &str, inner: &str) -> Vec<u8> {
    let signature = DetachedSignature::sign_binary_data(
        rand::thread_rng(),
        &secret.primary_key,
        &Password::empty(),
        HashAlgorithm::Sha256,
        inner.as_bytes(),
    )
    .unwrap()
    .to_armored_string(ArmorOptions::default())
    .unwrap();
    format!(
        "{outer}Subject: x\r\nMIME-Version: 1.0\r\n\
Content-Type: multipart/signed; micalg=pgp-sha256; protocol=\"application/pgp-signature\"; boundary=\"b\"\r\n\r\n\
--b\r\n{inner}\r\n--b\r\nContent-Type: application/pgp-signature\r\n\r\n{}\r\n--b--\r\n",
        signature.replace('\n', "\r\n")
    )
    .into_bytes()
}

#[test]
fn signer_is_bound_to_the_displayed_from_only() {
    let carol = secret_fixture("carol.sec.asc");
    let env = Env::with(&["carol.pub.asc"]);
    let ceo = "From: CEO <ceo@bank.example>\r\n";

    // A From inside the signed part without a header-protection marker is
    // not what the reader shows: the signer does not match.
    let raw = signed_part_with(
        &carol,
        ceo,
        "From: carol@example.com\r\nContent-Type: text/plain; charset=us-ascii\r\n\r\nSigned text.",
    );
    let outcome = env.read(&raw);
    let status = outcome.status.unwrap();
    let sig = &status.signatures[0];
    assert_eq!(sig.state, SignatureState::Valid);
    assert_eq!(sig.signer.as_deref(), Some("carol@example.com"));
    assert!(!sig.signer_matches_from, "{sig:?}");
    assert_eq!(status.header_protection, HeaderProtection::None);
    let display = String::from_utf8(outcome.display.unwrap()).unwrap();
    assert!(display.contains("From: CEO <ceo@bank.example>"));

    // With protected headers the reader shows the signed From: it matches
    // (and the disagreement is reported as a mismatch).
    let raw = signed_part_with(
        &carol,
        ceo,
        "From: carol@example.com\r\nContent-Type: text/plain; charset=us-ascii; protected-headers=\"v1\"\r\n\r\nSigned text.",
    );
    let outcome = env.read(&raw);
    let status = outcome.status.unwrap();
    assert!(status.signatures[0].signer_matches_from);
    assert_eq!(status.header_protection, HeaderProtection::Mismatch);
    let display = String::from_utf8(outcome.display.unwrap()).unwrap();
    assert!(display.contains("From: carol@example.com") && !display.contains("ceo@bank"));

    // Several From addresses: only the first counts (as for S/MIME).
    let inner = "Content-Type: text/plain; charset=us-ascii\r\n\r\nSigned text.";
    let first = signed_part_with(
        &carol,
        "From: carol@example.com, ceo@bank.example\r\n",
        inner,
    );
    assert!(env.status(&first).signatures[0].signer_matches_from);
    let second = signed_part_with(
        &carol,
        "From: ceo@bank.example, carol@example.com\r\n",
        inner,
    );
    assert!(!env.status(&second).signatures[0].signer_matches_from);
}

#[test]
fn encrypted_signer_is_bound_to_the_outer_from_without_protected_headers() {
    use pgp::composed::MessageBuilder;
    use pgp::crypto::sym::SymmetricKeyAlgorithm;
    let carol = secret_fixture("carol.sec.asc");
    let env = Env::with(&["bob.sec.asc", "carol.pub.asc"]);
    let bob_stored = env
        .store
        .keys_for_email("bob@example.net", None)
        .unwrap()
        .remove(0);
    let bob = env.keyring().public_key(&bob_stored).unwrap();
    let encrypt_to = |inner: &str| {
        let mut rng = rand::thread_rng();
        let mut builder = MessageBuilder::from_bytes("", inner.as_bytes().to_vec())
            .seipd_v1(&mut rng, SymmetricKeyAlgorithm::AES256);
        builder
            .encrypt_to_key(&mut rng, &bob.public_subkeys[0].key)
            .unwrap();
        builder.sign(&carol.primary_key, Password::empty(), HashAlgorithm::Sha256);
        let armored = builder
            .to_armored_string(&mut rng, ArmorOptions::default())
            .unwrap();
        format!(
            "From: CEO <ceo@bank.example>\r\nSubject: x\r\nMIME-Version: 1.0\r\n\
Content-Type: multipart/encrypted; protocol=\"application/pgp-encrypted\"; boundary=\"e\"\r\n\r\n\
--e\r\nContent-Type: application/pgp-encrypted\r\n\r\nVersion: 1\r\n\r\n\
--e\r\nContent-Type: application/octet-stream\r\n\r\n{}\r\n--e--\r\n",
            armored.replace('\n', "\r\n")
        )
        .into_bytes()
    };
    let raw = encrypt_to("From: carol@example.com\r\nContent-Type: text/plain\r\n\r\nhi");
    let status = env.status(&raw);
    assert_eq!(
        status.encryption.as_ref().unwrap().state,
        DecryptionState::Decrypted
    );
    let sig = &status.signatures[0];
    assert_eq!(sig.state, SignatureState::Valid);
    assert!(!sig.signer_matches_from, "{sig:?}");
    // Protected headers make the inner From the displayed one.
    let raw = encrypt_to(
        "From: carol@example.com\r\nContent-Type: text/plain; protected-headers=\"v1\"\r\n\r\nhi",
    );
    assert!(env.status(&raw).signatures[0].signer_matches_from);
}

fn secret_fixture(name: &str) -> pgp::composed::SignedSecretKey {
    let (keys, _) = parse_keys(&fixture(name));
    match keys.into_iter().next().unwrap() {
        ParsedKey::Secret(k) => *k,
        ParsedKey::Public(_) => panic!("{name} is not a secret key"),
    }
}

#[test]
fn signature_by_an_expired_key() {
    // dave's key expired in 2021; a signature made now is key-expired.
    let env = Env::with(&["dave.pub.asc"]);
    let raw = raw_signed_with(&secret_fixture("dave.sec.asc"));
    let status = env.status(&raw);
    assert_eq!(status.signatures[0].state, SignatureState::KeyExpired);
    assert!(status.signatures[0].signer_matches_from);

    // Nail itself refuses to sign or encrypt with it.
    let env = Env::with(&["dave.sec.asc"]);
    let dave = env
        .keyring()
        .list(&nail_api::security::KeyFilter::default())
        .unwrap()
        .remove(0);
    let key = env
        .keyring()
        .at(NOW)
        .unlocked_key(&env.store.get_key(&dave.id).unwrap())
        .unwrap();
    let result = protect_message(
        PLAIN,
        &Protection {
            signer: Some(&key),
            recipients: &[],
            protect_headers: false,
            gossip: &[],
            now: NOW,
        },
    );
    assert!(matches!(result, Err(Error::NoKey(_))), "{result:?}");
    let dave_public = public(&env, &dave.id);
    let result = protect_message(
        PLAIN,
        &Protection {
            signer: None,
            recipients: &[dave_public],
            protect_headers: false,
            gossip: &[],
            now: NOW,
        },
    );
    assert!(matches!(result, Err(Error::NoKey(_))), "{result:?}");
}

#[test]
fn signature_by_a_revoked_key() {
    let secret = secret_fixture("bob.sec.asc");
    let raw = raw_signed_with(&secret);
    let env = Env::with(&["bob.pub.asc"]);
    assert_eq!(env.status(&raw).signatures[0].state, SignatureState::Valid);

    // Revoke bob's key and import the revocation.
    let primary = &secret.primary_key;
    let mut config =
        SignatureConfig::from_key(rand::thread_rng(), primary, SignatureType::KeyRevocation)
            .unwrap();
    config.hashed_subpackets = vec![
        Subpacket::regular(SubpacketData::SignatureCreationTime(
            pgp::types::Timestamp::now(),
        ))
        .unwrap(),
        Subpacket::regular(SubpacketData::IssuerFingerprint(primary.fingerprint())).unwrap(),
    ];
    let revocation = config
        .sign_key(primary, &Password::empty(), primary.public_key())
        .unwrap();
    let mut revoked = secret.to_public_key();
    revoked.details.revocation_signatures.push(revocation);
    env.keyring()
        .store_public(&revoked, nail_api::security::KeySource::Imported)
        .unwrap();
    let status = env.status(&raw);
    assert_eq!(status.signatures[0].state, SignatureState::KeyRevoked);
}

#[test]
fn protected_headers_that_disagree_are_a_mismatch() {
    let p = pair();
    let signed = sign_only(&p.ada, &p.ada_key, true);
    assert_eq!(
        p.bob.status(&signed).header_protection,
        HeaderProtection::Protected
    );
    // Someone rewrites the outer From: the signature still covers the
    // content, but the protected From disagrees.
    let forged = String::from_utf8(signed).unwrap().replacen(
        "From: Ada Lovelace <ada@example.org>",
        "From: Mallory <mallory@example.org>",
        1,
    );
    let outcome = p.bob.read(forged.as_bytes());
    let status = outcome.status.unwrap();
    assert_eq!(status.header_protection, HeaderProtection::Mismatch);
    assert_eq!(status.signatures[0].state, SignatureState::Valid);
    // The display shows the protected (signed) From.
    let display = String::from_utf8(outcome.display.unwrap()).unwrap();
    assert!(display.contains("From: Ada Lovelace <ada@example.org>"));
    assert!(!display.contains("mallory"));
}

#[test]
fn mailing_list_footer_makes_the_signature_partial() {
    let p = pair();
    let signed = sign_only(&p.ada, &p.ada_key, false);
    let text = String::from_utf8(signed).unwrap();
    let (head, body) = text.split_once("\r\n\r\n").unwrap();
    let content_type_start = head.find("Content-Type:").unwrap();
    let outer_head = &head[..content_type_start];
    let signed_entity = format!("{}\r\n\r\n{}", &head[content_type_start..], body);
    let wrapped = format!(
        "{outer_head}Content-Type: multipart/mixed; boundary=\"list\"\r\n\r\n--list\r\n{signed_entity}\r\n--list\r\nContent-Type: text/plain\r\n\r\nList footer\r\n--list--\r\n"
    );
    let outcome = p.bob.read(wrapped.as_bytes());
    let status = outcome.status.unwrap();
    assert!(status.partial);
    assert_eq!(status.signatures[0].state, SignatureState::Valid);
    assert!(outcome.display.is_none());
}

#[test]
fn nested_sign_then_encrypt() {
    // RFC 3156 §6.1: a multipart/signed entity inside multipart/encrypted.
    let p = pair();
    let signed = sign_only(&p.ada, &p.ada_key, false);
    let recipients = vec![peer_public(&p.ada, &p.bob, &p.bob_key)];
    let sent = protect_message(
        &signed,
        &Protection {
            signer: None,
            recipients: &recipients,
            protect_headers: false,
            gossip: &[],
            now: now(),
        },
    )
    .unwrap();
    let outcome = p.bob.read(&sent);
    let status = outcome.status.unwrap();
    assert_eq!(status.encryption.unwrap().state, DecryptionState::Decrypted);
    assert_eq!(status.signatures.len(), 1);
    assert_eq!(status.signatures[0].state, SignatureState::Valid);
    let (_, body) = subject_and_text(&outcome.display.unwrap());
    assert!(body.contains("The analytical engine"));
}

#[test]
fn locked_passphrase_key_cannot_sign_until_unlocked() {
    let env = Env::new();
    let key = env
        .keyring()
        .generate(
            "Ada",
            "ada@example.org",
            KeyAlgorithm::Curve25519,
            None,
            &SecretProtection::Passphrase {
                passphrase: "open sesame".into(),
            },
        )
        .unwrap();
    let stored = env.store.get_key(&key.id).unwrap();
    assert!(matches!(
        env.keyring().unlocked_key(&stored),
        Err(Error::Locked { .. })
    ));
    env.keyring()
        .unlock(&key.id, "open sesame", Some(1))
        .unwrap();
    let signer = env.keyring().unlocked_key(&stored).unwrap();
    let signed_message = protect_message(
        PLAIN,
        &Protection {
            signer: Some(&signer),
            recipients: &[],
            protect_headers: false,
            gossip: &[],
            now: now(),
        },
    )
    .unwrap();
    assert_eq!(
        env.status(&signed_message).signatures[0].state,
        SignatureState::Valid
    );
}
