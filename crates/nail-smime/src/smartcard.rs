//! S/MIME keys on PKCS#11 smartcards (M4c WP122, ADR 0095).
//!
//! [`SmartcardKey`] implements [`PrivateKeyOps`] over a
//! [`nail_pkcs11::CardKey`]: signatures (RSA PKCS#1 v1.5 over a
//! `DigestInfo`, RSA-PSS, ECDSA converted to DER), RSA key transport
//! (PKCS#1 v1.5 and OAEP) and ECDH key agreement happen on the card; a
//! missing login asks for the PIN through the card's prompter. The
//! keyring stores such keys with [`nail_api::security::SecretStorage::Smartcard`]
//! and the JSON of the [`SmartcardKeyRef`] as the secret reference
//! ([`crate::keyring::Keyring::use_smartcard_key`]).

use nail_api::smartcard::SmartcardKeyRef;
use nail_pkcs11::{CardError, CardKey, DecryptMechanism, HashKind, SignMechanism};
use zeroize::Zeroizing;

use crate::cert::Cert;
use crate::error::{Error, Result};
use crate::keys::{HashAlg, PrivateKeyOps, RsaPadding, SignScheme};

fn kind(hash: HashAlg) -> HashKind {
    match hash {
        HashAlg::Sha1 => HashKind::Sha1,
        HashAlg::Sha256 => HashKind::Sha256,
        HashAlg::Sha384 => HashKind::Sha384,
        HashAlg::Sha512 => HashKind::Sha512,
    }
}

/// A private key that stays on a smartcard.
#[derive(Debug, Clone)]
pub struct SmartcardKey {
    card: CardKey,
}

impl SmartcardKey {
    /// Wraps a card key.
    pub fn new(card: CardKey) -> Self {
        Self { card }
    }

    /// The card key.
    pub fn card(&self) -> &CardKey {
        &self.card
    }
}

impl PrivateKeyOps for SmartcardKey {
    fn sign_digest(&self, scheme: SignScheme, digest: &[u8]) -> Result<Vec<u8>> {
        match scheme {
            SignScheme::RsaPkcs1(hash) => {
                let input = nail_pkcs11::digest_info(kind(hash), digest)?;
                Ok(self.card.sign(SignMechanism::RsaPkcs1, &input)?)
            }
            SignScheme::RsaPss { hash, salt_len } => {
                if salt_len != kind(hash).len() {
                    return Err(Error::Unsupported(format!(
                        "RSA-PSS with a {salt_len}-byte salt on a smartcard"
                    )));
                }
                Ok(self.card.sign(SignMechanism::RsaPss(kind(hash)), digest)?)
            }
            SignScheme::Ecdsa(_) => {
                let raw = self.card.sign(SignMechanism::Ecdsa, digest)?;
                Ok(nail_pkcs11::ecdsa_raw_to_der(&raw)?)
            }
        }
    }

    fn decrypt_rsa(&self, padding: RsaPadding, ciphertext: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        let mechanism = match padding {
            RsaPadding::Pkcs1v15 => DecryptMechanism::RsaPkcs1,
            RsaPadding::Oaep(hash) => DecryptMechanism::RsaOaep(kind(hash)),
        };
        Ok(self.card.decrypt(mechanism, ciphertext)?)
    }

    fn ecdh(&self, peer_point: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        Ok(self.card.ecdh(peer_point)?)
    }
}

/// Whether a certificate is for the card key's public key (when the card
/// shows it; a card that shows nothing is taken at its word).
pub fn certificate_matches(cert: &Cert, card: &CardKey) -> bool {
    let Some(on_card) = card.public_key() else {
        return true;
    };
    let in_cert =
        nail_pkcs11::public_key_of_spki(&cert.inner().tbs_certificate.subject_public_key_info);
    in_cert.is_some_and(|c| on_card.same_public_key(&c))
}

/// The secret reference stored for a smartcard key.
pub fn reference_of(key: &SmartcardKeyRef) -> String {
    serde_json::to_string(key).unwrap_or_default()
}

/// Parses [`reference_of`].
pub fn parse_reference(reference: &str) -> Result<SmartcardKeyRef> {
    serde_json::from_str(reference)
        .map_err(|e| Error::malformed(format!("smartcard key reference: {e}")))
}

impl From<CardError> for Error {
    fn from(err: CardError) -> Self {
        Self::Smartcard(err)
    }
}

#[cfg(test)]
mod tests {
    //! S/MIME with keys on the soft token: adding the key, signing and
    //! decrypting on the card (RSA and P-256), PIN prompts, a removed card.

    use std::sync::Arc;

    use nail_api::engine::MemorySecretStore;
    use nail_api::security::{DecryptionState, KeySource, SecretStorage, SignatureState};
    use nail_api::smartcard::{PinPurpose, SmartcardError};
    use nail_pkcs11::testing::{SoftKey, SoftToken};
    use nail_pkcs11::{FixedPin, Module, Smartcards};
    use nail_store::Store;
    use rsa::pkcs8::DecodePrivateKey;

    use super::*;
    use crate::keyring::{Keyring, UnlockCache};
    use crate::mime::{SignMode, encrypt_entity, sign_entity};
    use crate::testutil::{AT, fixture, fixture_cert, test_anchors};
    use crate::{ContentCipher, chain};

    const MODULE: &str = r"C:\test\soft-pkcs11.dll";
    const PIN: &str = "4711";

    fn pem_key(name: &str) -> String {
        String::from_utf8(fixture(name)).unwrap()
    }

    struct Env {
        store: Store,
        secrets: MemorySecretStore,
        anchors: chain::FixedTrustAnchors,
        cache: UnlockCache,
        token: Arc<SoftToken>,
        pins: Arc<FixedPin>,
        cards: Smartcards,
    }

    impl Env {
        /// Alice's RSA key with her certificate on the card, Bob's P-256
        /// key without a certificate.
        fn new(pins: &[Option<&'static str>]) -> Self {
            let token = Arc::new(SoftToken::new(MODULE));
            token.add_token(3, "PIV card", "PIV-0042", PIN);
            let alice = rsa::RsaPrivateKey::from_pkcs8_pem(&pem_key("alice.key")).unwrap();
            token.add_key(
                3,
                &[0x9c],
                "Alice signature",
                SoftKey::Rsa(Box::new(alice)),
                Some(fixture_cert("alice.crt").der().to_vec()),
                false,
            );
            let bob = p256::SecretKey::from_pkcs8_pem(&pem_key("bob.key")).unwrap();
            token.add_key(3, &[0x9d], "Bob key", SoftKey::P256(bob), None, true);
            let pins = Arc::new(FixedPin::answers(pins.iter().copied()));
            let module: Arc<dyn Module> = token.clone();
            let cards = Smartcards::from_modules(vec![module], 10, pins.clone());
            Self {
                store: Store::open_in_memory().unwrap(),
                secrets: MemorySecretStore::new(),
                anchors: test_anchors(),
                cache: UnlockCache::new(),
                token,
                pins,
                cards,
            }
        }

        fn keyring(&self) -> Keyring<'_> {
            Keyring::new(&self.store, &self.secrets, &self.anchors, &self.cache)
                .with_smartcards(&self.cards)
                .at(AT)
        }
    }

    fn key_ref(id: &str) -> SmartcardKeyRef {
        SmartcardKeyRef {
            module: MODULE.into(),
            token_serial: "PIV-0042".into(),
            object_id: id.into(),
        }
    }

    #[test]
    fn rsa_key_on_the_card_signs_and_decrypts() {
        let env = Env::new(&[Some(PIN)]);
        let keyring = env.keyring();
        let result = keyring.use_smartcard_key(&key_ref("9c")).unwrap();
        assert_eq!(result.created, 1);
        let key = &result.keys[0];
        assert_eq!(key.secret_storage, Some(SecretStorage::Smartcard));
        assert_eq!(key.source, KeySource::Smartcard);
        assert!(key.has_secret);
        assert_eq!(key.smartcard.as_ref(), Some(&key_ref("9c")));
        assert_eq!(key.emails, ["alice@example.org"]);
        // Adding it again updates the same entry.
        assert_eq!(
            keyring.use_smartcard_key(&key_ref("9c")).unwrap().updated,
            1
        );

        // Signing happens on the card (one PIN prompt for the session).
        let signer = keyring.signer("alice@example.org", None).unwrap();
        let signed_mail = sign_entity(
            &fixture("content.eml"),
            &signer.signer(),
            SignMode::Detached,
            AT,
        )
        .unwrap();
        let outcome = keyring
            .read_message(&signed_mail, Some("alice@example.org"))
            .unwrap()
            .unwrap();
        assert_eq!(outcome.status.signatures[0].state, SignatureState::Valid);
        let requests = env.pins.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].purpose, PinPurpose::Sign);

        // Mail encrypted to Alice is decrypted by the card (RSA key
        // transport), within the PIN cache: no second prompt.
        let alice_cert = fixture_cert("alice.crt");
        let encrypted = encrypt_entity(
            &fixture("content.eml"),
            &[alice_cert],
            ContentCipher::Aes256Gcm,
        )
        .unwrap();
        let outcome = keyring.read_message(&encrypted, None).unwrap().unwrap();
        let enc = outcome.status.encryption.unwrap();
        assert_eq!(enc.state, DecryptionState::Decrypted);
        assert_eq!(enc.key_id, Some(key.id.clone()));
        assert_eq!(env.pins.requests().len(), 1);
        assert_eq!(env.token.stats().operations, 2);
    }

    #[test]
    fn ec_key_finds_its_certificate_in_the_keyring() {
        let env = Env::new(&[Some(PIN)]);
        let keyring = env.keyring();
        // No certificate on the card and none in the keyring yet.
        assert!(matches!(
            keyring.use_smartcard_key(&key_ref("9d")),
            Err(Error::NoKey(_))
        ));
        keyring
            .import_certificates(&fixture("bob.crt"), KeySource::Imported)
            .unwrap();
        let key = keyring
            .use_smartcard_key(&key_ref("9d"))
            .unwrap()
            .keys
            .remove(0);
        assert_eq!(key.smartcard.as_ref(), Some(&key_ref("9d")));

        let signer = keyring.signer("bob@example.net", None).unwrap();
        let signed_mail = sign_entity(
            &fixture("content.eml"),
            &signer.signer(),
            SignMode::Opaque,
            AT,
        )
        .unwrap();
        let outcome = keyring
            .read_message(&signed_mail, Some("bob@example.net"))
            .unwrap()
            .unwrap();
        assert_eq!(outcome.status.signatures[0].state, SignatureState::Valid);
        // ECDH key agreement on the card.
        let encrypted = encrypt_entity(
            &fixture("content.eml"),
            &[fixture_cert("bob.crt")],
            ContentCipher::Aes128Cbc,
        )
        .unwrap();
        let outcome = keyring.read_message(&encrypted, None).unwrap().unwrap();
        assert_eq!(
            outcome.status.encryption.unwrap().state,
            DecryptionState::Decrypted
        );
    }

    #[test]
    fn missing_cards_cancelled_pins_and_foreign_certificates() {
        let env = Env::new(&[None]);
        let keyring = env.keyring();
        keyring.use_smartcard_key(&key_ref("9c")).unwrap();
        let signer = keyring.signer("alice@example.org", None).unwrap();
        let err = sign_entity(
            &fixture("content.eml"),
            &signer.signer(),
            SignMode::Detached,
            AT,
        )
        .unwrap_err();
        assert!(matches!(&err, Error::Smartcard(e) if e.kind == SmartcardError::PinCancelled));
        let api: nail_api::ApiError = err.into();
        assert_eq!(api.localized.unwrap().id, "smartcard-error-pin-cancelled");

        env.token.set_present(3, false);
        assert!(matches!(
            keyring.signer("alice@example.org", None),
            Err(Error::Smartcard(e)) if e.kind == SmartcardError::TokenNotPresent
        ));
        // Without smartcard support the key is unsupported, not broken.
        let plain = Keyring::new(&env.store, &env.secrets, &env.anchors, &env.cache).at(AT);
        assert!(matches!(
            plain.signer("alice@example.org", None),
            Err(Error::Unsupported(_))
        ));
        assert!(matches!(
            plain.use_smartcard_key(&key_ref("9c")),
            Err(Error::Unsupported(_))
        ));

        // A certificate for another key is refused.
        let env = Env::new(&[]);
        let token = SoftToken::new(r"C:\test\other.dll");
        token.add_token(1, "Card", "X1", PIN);
        let alice = rsa::RsaPrivateKey::from_pkcs8_pem(&pem_key("alice.key")).unwrap();
        token.add_key(
            1,
            &[1],
            "Mixed up",
            SoftKey::Rsa(Box::new(alice)),
            Some(fixture_cert("dave.crt").der().to_vec()),
            true,
        );
        let module: Arc<dyn Module> = Arc::new(token);
        let cards = Smartcards::from_modules(vec![module], 0, env.pins.clone());
        let keyring = Keyring::new(&env.store, &env.secrets, &env.anchors, &env.cache)
            .with_smartcards(&cards)
            .at(AT);
        let mixed = SmartcardKeyRef {
            module: r"C:\test\other.dll".into(),
            token_serial: "X1".into(),
            object_id: "01".into(),
        };
        assert!(matches!(
            keyring.use_smartcard_key(&mixed),
            Err(Error::Crypto(_))
        ));
    }

    #[test]
    fn references_round_trip() {
        let key = key_ref("9c");
        assert_eq!(parse_reference(&reference_of(&key)).unwrap(), key);
        assert!(parse_reference("{").is_err());
    }
}
