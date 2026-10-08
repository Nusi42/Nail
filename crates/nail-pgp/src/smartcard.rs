//! OpenPGP keys on PKCS#11 smartcards (M4c WP122, ADR 0095).
//!
//! rPGP lets external signers in through its [`SigningKey`] trait:
//! [`CardSigningKey`] pairs a public (sub)key packet from the keyring with
//! a [`CardKey`] and signs on the card — RSA (PKCS#1 v1.5 over the
//! `DigestInfo`) and ECDSA on NIST curves. Decryption is done outside
//! rPGP's key types: [`card_session_key`] decrypts an RSA session-key
//! packet on the card and the message is then opened with the plain
//! session key. EdDSA / Curve25519 keys on cards are not supported (no
//! portable PKCS#11 mechanism for OpenPGP's legacy formats); they report
//! [`nail_api::smartcard::SmartcardError::MechanismUnsupported`].
//!
//! The keyring stores card keys with `SecretStorage::Smartcard`; the
//! secret reference is the JSON of one [`SmartcardKeyRef`] or, when the
//! signing and decryption subkeys are separate card keys (an OpenPGP
//! card), a JSON array of them ([`parse_references`]).

use nail_api::smartcard::{SmartcardError, SmartcardKeyRef};
use nail_pkcs11::{
    CardError, CardKey, DecryptMechanism, HashKind, PublicKeyMaterial, SignMechanism,
};
use pgp::composed::{PlainSessionKey, SignedPublicKey};
use pgp::crypto::hash::HashAlgorithm;
use pgp::crypto::public_key::PublicKeyAlgorithm;
use pgp::crypto::sym::SymmetricKeyAlgorithm;
use pgp::packet::{PublicKey, PublicKeyEncryptedSessionKey, PublicSubkey};
use pgp::types::{
    Fingerprint, KeyDetails, KeyId as PgpKeyId, KeyVersion, Mpi, Password, PkeskBytes,
    PkeskVersion, PublicParams, SignatureBytes, SigningKey, Timestamp,
};
use zeroize::Zeroizing;

use crate::error::{Error, Result};

/// A public key component of an OpenPGP key.
#[derive(Debug, Clone)]
pub enum Component {
    /// The primary key.
    Primary(PublicKey),
    /// A subkey.
    Subkey(PublicSubkey),
}

impl Component {
    fn details(&self) -> &dyn KeyDetails {
        match self {
            Self::Primary(k) => k,
            Self::Subkey(k) => k,
        }
    }
}

/// The public material of a component, comparable with a card key's.
fn material(params: &PublicParams) -> Option<PublicKeyMaterial> {
    use p256::elliptic_curve::sec1::ToEncodedPoint;
    use rsa::traits::PublicKeyParts;
    match params {
        PublicParams::RSA(p) => Some(PublicKeyMaterial::Rsa {
            modulus: p.key.n().to_bytes_be(),
            exponent: p.key.e().to_bytes_be(),
        }),
        PublicParams::ECDSA(pgp::types::EcdsaPublicParams::P256 { key }) => {
            Some(PublicKeyMaterial::Ec {
                params: vec![0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07],
                point: key.to_encoded_point(false).as_bytes().to_vec(),
            })
        }
        PublicParams::ECDSA(pgp::types::EcdsaPublicParams::P384 { key }) => {
            Some(PublicKeyMaterial::Ec {
                params: vec![0x06, 0x05, 0x2b, 0x81, 0x04, 0x00, 0x22],
                point: key.to_encoded_point(false).as_bytes().to_vec(),
            })
        }
        _ => None,
    }
}

/// Whether a card key's public material is this OpenPGP component's.
pub fn same_key(params: &PublicParams, card: &PublicKeyMaterial) -> bool {
    material(params).is_some_and(|m| m.same_public_key(card))
}

/// The component of `key` whose public key is the card key's, with its
/// fingerprint (upper-case hex): what `SmartcardObject.openpgp_fingerprint`
/// shows and what `UseSmartcardKey` attaches the card key to.
pub fn matching_component(
    key: &SignedPublicKey,
    card: &PublicKeyMaterial,
) -> Option<(Component, String)> {
    let hex = |fp: Fingerprint| hex::encode_upper(fp.as_bytes());
    if same_key(key.primary_key.public_params(), card) {
        return Some((
            Component::Primary(key.primary_key.clone()),
            hex(key.primary_key.fingerprint()),
        ));
    }
    key.public_subkeys
        .iter()
        .find(|s| same_key(s.key.public_params(), card))
        .map(|s| (Component::Subkey(s.key.clone()), hex(s.key.fingerprint())))
}

/// The secret reference stored for one or more card keys.
pub fn reference_of(keys: &[SmartcardKeyRef]) -> String {
    match keys {
        [one] => serde_json::to_string(one),
        many => serde_json::to_string(many),
    }
    .unwrap_or_default()
}

/// Parses [`reference_of`] (one object or an array).
pub fn parse_references(reference: &str) -> Result<Vec<SmartcardKeyRef>> {
    if let Ok(one) = serde_json::from_str::<SmartcardKeyRef>(reference) {
        return Ok(vec![one]);
    }
    serde_json::from_str::<Vec<SmartcardKeyRef>>(reference)
        .map_err(|e| Error::Invalid(format!("smartcard key reference: {e}")))
}

fn pgp_error(err: impl std::fmt::Display) -> pgp::errors::Error {
    pgp::errors::Error::Message {
        message: err.to_string(),
        backtrace: None,
    }
}

fn hash_kind(hash: HashAlgorithm) -> Option<HashKind> {
    Some(match hash {
        HashAlgorithm::Sha1 => HashKind::Sha1,
        HashAlgorithm::Sha256 => HashKind::Sha256,
        HashAlgorithm::Sha384 => HashKind::Sha384,
        HashAlgorithm::Sha512 => HashKind::Sha512,
        _ => return None,
    })
}

/// A signing (sub)key whose secret is on a card. A failed card operation
/// is reported to rPGP as an error; [`CardSigningKey::last_error`] keeps
/// the card's class for Nail's own error.
#[derive(Debug)]
pub struct CardSigningKey {
    component: Component,
    card: CardKey,
    last_error: std::sync::Mutex<Option<CardError>>,
}

impl CardSigningKey {
    /// Pairs a public component with its card key.
    pub fn new(component: Component, card: CardKey) -> Self {
        Self {
            component,
            card,
            last_error: std::sync::Mutex::new(None),
        }
    }

    /// The card error of the last failed signature, if any.
    pub fn last_error(&self) -> Option<CardError> {
        self.last_error
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn fail(&self, err: CardError) -> pgp::errors::Error {
        let out = pgp_error(&err);
        *self
            .last_error
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(err);
        out
    }
}

impl KeyDetails for CardSigningKey {
    fn version(&self) -> KeyVersion {
        self.component.details().version()
    }
    fn legacy_key_id(&self) -> PgpKeyId {
        self.component.details().legacy_key_id()
    }
    fn fingerprint(&self) -> Fingerprint {
        self.component.details().fingerprint()
    }
    fn algorithm(&self) -> PublicKeyAlgorithm {
        self.component.details().algorithm()
    }
    fn created_at(&self) -> Timestamp {
        self.component.details().created_at()
    }
    fn legacy_v3_expiration_days(&self) -> Option<u16> {
        self.component.details().legacy_v3_expiration_days()
    }
    fn public_params(&self) -> &PublicParams {
        self.component.details().public_params()
    }
}

impl SigningKey for CardSigningKey {
    fn sign(
        &self,
        _key_pw: &Password,
        hash: HashAlgorithm,
        data: &[u8],
    ) -> pgp::errors::Result<SignatureBytes> {
        match self.public_params() {
            PublicParams::RSA(_) => {
                let kind = hash_kind(hash).ok_or_else(|| {
                    self.fail(CardError::new(
                        SmartcardError::MechanismUnsupported,
                        format!("hash {hash:?} on a smartcard"),
                    ))
                })?;
                let input = nail_pkcs11::digest_info(kind, data).map_err(|e| self.fail(e))?;
                let sig = self
                    .card
                    .sign(SignMechanism::RsaPkcs1, &input)
                    .map_err(|e| self.fail(e))?;
                Ok(SignatureBytes::Mpis(vec![Mpi::from_slice(&sig)]))
            }
            PublicParams::ECDSA(_) => {
                let raw = self
                    .card
                    .sign(SignMechanism::Ecdsa, data)
                    .map_err(|e| self.fail(e))?;
                if raw.is_empty() || !raw.len().is_multiple_of(2) {
                    return Err(self.fail(CardError::new(
                        SmartcardError::DeviceError,
                        "malformed ECDSA signature from the card",
                    )));
                }
                let (r, s) = raw.split_at(raw.len() / 2);
                Ok(SignatureBytes::Mpis(vec![
                    Mpi::from_slice(r),
                    Mpi::from_slice(s),
                ]))
            }
            other => Err(self.fail(CardError::new(
                SmartcardError::MechanismUnsupported,
                format!("OpenPGP {other:?} keys on smartcards"),
            ))),
        }
    }

    fn hash_alg(&self) -> HashAlgorithm {
        match self.public_params() {
            PublicParams::ECDSA(pgp::types::EcdsaPublicParams::P384 { .. }) => {
                HashAlgorithm::Sha384
            }
            _ => HashAlgorithm::Sha256,
        }
    }
}

/// Decrypts a session-key packet with an RSA key on the card; `modulus_len`
/// is the key's modulus length in bytes (the ciphertext is left-padded to
/// it, as PKCS#11 expects).
pub fn card_session_key(
    card: &CardKey,
    pkesk: &PublicKeyEncryptedSessionKey,
    modulus_len: usize,
) -> Result<PlainSessionKey> {
    let PkeskBytes::Rsa { mpi } = pkesk.values().map_err(Error::crypto)? else {
        return Err(CardError::new(
            SmartcardError::MechanismUnsupported,
            "only RSA session keys are decrypted on smartcards",
        )
        .into());
    };
    let bytes: &[u8] = mpi.as_ref();
    let mut ciphertext = vec![0u8; modulus_len.saturating_sub(bytes.len())];
    ciphertext.extend_from_slice(bytes);
    let plain: Zeroizing<Vec<u8>> = card.decrypt(DecryptMechanism::RsaPkcs1, &ciphertext)?;
    let bad = || Error::Crypto("the decrypted session key is malformed".into());
    let (body, check) = plain.split_at(plain.len().checked_sub(2).ok_or_else(bad)?);
    let (sym_alg, key) = match pkesk.version() {
        PkeskVersion::V3 => {
            let (&alg, key) = body.split_first().ok_or_else(bad)?;
            (Some(SymmetricKeyAlgorithm::from(alg)), key)
        }
        PkeskVersion::V6 => (None, body),
        PkeskVersion::Other(v) => {
            return Err(Error::Unsupported(format!(
                "session key packet version {v}"
            )));
        }
    };
    let sum = key
        .iter()
        .fold(0u16, |acc, b| acc.wrapping_add(u16::from(*b)));
    if sum.to_be_bytes() != [check[0], check[1]] {
        return Err(bad());
    }
    Ok(match sym_alg {
        Some(sym_alg) => PlainSessionKey::V3_4 {
            sym_alg,
            key: key.to_vec().into(),
        },
        None => PlainSessionKey::V6 {
            key: key.to_vec().into(),
        },
    })
}

#[cfg(test)]
mod tests {
    //! OpenPGP with keys on the soft token: an RSA key whose primary key
    //! (signing) and subkey (encryption) live on the card, the public key
    //! in the keyring. Signing and decrypting happen on the card.

    use std::sync::{Arc, OnceLock};

    use nail_api::KeyId;
    use nail_api::engine::MemorySecretStore;
    use nail_api::security::{
        DecryptionState, KeyAlgorithm, KeySource, SecretStorage, SignatureState,
    };
    use nail_pkcs11::testing::{SoftKey, SoftToken};
    use nail_pkcs11::{FixedPin, Module, Smartcards};
    use nail_store::Store;
    use pgp::composed::SignedSecretKey;
    use pgp::types::PlainSecretParams;

    use super::*;
    use crate::compose::{Protection, protect_message};
    use crate::keyring::{Keyring, UnlockCache, generate_secret};
    use crate::read::read_message;

    const MODULE: &str = r"C:\test\openpgp-card.dll";
    use nail_pkcs11::testing::TEST_PIN as PIN;
    const PLAIN: &[u8] = b"From: Ada <ada@example.org>\r\nTo: Bob <bob@example.net>\r\n\
Subject: Card mail\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nSigned on the card.\r\n";

    /// One RSA 2048 key for all tests (generation is slow in debug builds).
    fn secret() -> &'static SignedSecretKey {
        static KEY: OnceLock<SignedSecretKey> = OnceLock::new();
        KEY.get_or_init(|| {
            generate_secret("Ada <ada@example.org>", KeyAlgorithm::Rsa4096, 2048, None).unwrap()
        })
    }

    fn rsa_of(public: &PublicParams, plain: &PlainSecretParams) -> rsa::RsaPrivateKey {
        use rsa::BigUint;
        use rsa::traits::PublicKeyParts;
        let (PublicParams::RSA(public), PlainSecretParams::RSA(secret)) = (public, plain) else {
            panic!("an RSA key");
        };
        let (d, p, q, _) = secret.to_bytes();
        rsa::RsaPrivateKey::from_components(
            public.key.n().clone(),
            public.key.e().clone(),
            BigUint::from_bytes_be(&d),
            vec![BigUint::from_bytes_be(&p), BigUint::from_bytes_be(&q)],
        )
        .unwrap()
    }

    struct Env {
        store: Store,
        secrets: MemorySecretStore,
        cache: UnlockCache,
        token: Arc<SoftToken>,
        pins: Arc<FixedPin>,
        cards: Smartcards,
        key: KeyId,
    }

    impl Env {
        /// The card holds the primary key (id 01) and the subkey (id 02);
        /// the keyring the public key.
        fn new(pins: &[Option<&'static str>]) -> Self {
            let secret = secret();
            let token = Arc::new(SoftToken::new(MODULE));
            token.add_token(0, "OpenPGP card", "000612345678", PIN);
            let primary = secret
                .primary_key
                .unlock(&"".into(), |pub_params, plain| {
                    Ok(rsa_of(pub_params, plain))
                })
                .unwrap()
                .unwrap();
            let sub = secret.secret_subkeys[0]
                .key
                .unlock(&"".into(), |pub_params, plain| {
                    Ok(rsa_of(pub_params, plain))
                })
                .unwrap()
                .unwrap();
            token.add_key(
                0,
                &[1],
                "Signature key",
                SoftKey::Rsa(Box::new(primary)),
                None,
                true,
            );
            token.add_key(
                0,
                &[2],
                "Encryption key",
                SoftKey::Rsa(Box::new(sub)),
                None,
                true,
            );
            let pins = Arc::new(FixedPin::answers(pins.iter().copied()));
            let module: Arc<dyn Module> = token.clone();
            let cards = Smartcards::from_modules(vec![module], 10, pins.clone());
            let store = Store::open_in_memory().unwrap();
            let secrets = MemorySecretStore::new();
            let cache = UnlockCache::new();
            let public = crate::key::to_binary(&secret.to_public_key()).unwrap();
            let key = Keyring::new(&store, &secrets, &cache)
                .import_bytes(&public, KeySource::Imported)
                .unwrap()
                .keys[0]
                .id
                .clone();
            Self {
                store,
                secrets,
                cache,
                token,
                pins,
                cards,
                key,
            }
        }

        fn keyring(&self) -> Keyring<'_> {
            Keyring::new(&self.store, &self.secrets, &self.cache).with_smartcards(&self.cards)
        }

        fn attach_both(&self) {
            let ring = self.keyring();
            ring.use_smartcard_key(&key_ref("01")).unwrap();
            ring.use_smartcard_key(&key_ref("02")).unwrap();
        }
    }

    fn key_ref(id: &str) -> SmartcardKeyRef {
        SmartcardKeyRef {
            module: MODULE.into(),
            token_serial: "000612345678".into(),
            object_id: id.into(),
        }
    }

    fn now() -> i64 {
        chrono::Utc::now().timestamp()
    }

    #[test]
    fn card_keys_attach_to_the_keyring_key() {
        let env = Env::new(&[]);
        let ring = env.keyring();
        let card = env.cards.describe_key(&key_ref("02")).unwrap();
        let (key_id, fingerprint) = ring
            .fingerprint_for_card_key(card.public_key.as_ref().unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(key_id, env.key);
        let subkey = secret().secret_subkeys[0].key.fingerprint();
        assert_eq!(fingerprint, hex::encode_upper(subkey.as_bytes()));

        let key = ring.use_smartcard_key(&key_ref("01")).unwrap();
        assert_eq!(key.id, env.key);
        assert_eq!(key.secret_storage, Some(SecretStorage::Smartcard));
        assert_eq!(key.source, KeySource::Smartcard);
        assert!(key.has_secret);
        assert_eq!(key.smartcard.as_ref(), Some(&key_ref("01")));
        // The second card key joins the reference (once).
        ring.use_smartcard_key(&key_ref("02")).unwrap();
        ring.use_smartcard_key(&key_ref("02")).unwrap();
        let stored = env.store.get_key(&env.key).unwrap();
        let refs =
            parse_references(stored.data.secret.unwrap().reference.as_deref().unwrap()).unwrap();
        assert_eq!(refs, [key_ref("01"), key_ref("02")]);
        // A key the keyring does not have is refused.
        let other = SoftToken::new(r"C:\test\other.dll");
        other.add_token(1, "Other", "X", PIN);
        other.add_key(
            1,
            &[9],
            "x",
            SoftKey::Rsa(Box::new(nail_pkcs11::testing::test_rsa_key())),
            None,
            true,
        );
        let module: Arc<dyn Module> = Arc::new(other);
        let cards = Smartcards::from_modules(vec![module], 0, Arc::new(FixedPin::default()));
        let ring = Keyring::new(&env.store, &env.secrets, &env.cache).with_smartcards(&cards);
        let unknown = SmartcardKeyRef {
            module: r"C:\test\other.dll".into(),
            token_serial: "X".into(),
            object_id: "09".into(),
        };
        assert!(matches!(
            ring.use_smartcard_key(&unknown),
            Err(Error::NoKey(_))
        ));
    }

    #[test]
    fn signing_and_decrypting_on_the_card() {
        let env = Env::new(&[Some(PIN)]);
        env.attach_both();
        let ring = env.keyring();
        let stored = env.store.get_key(&env.key).unwrap();
        let signer = ring.unlocked_key(&stored).unwrap();
        assert!(signer.on_smartcard());
        assert!(signer.secret_key().is_err());
        let public = ring.public_key(&stored).unwrap();
        let sent = protect_message(
            PLAIN,
            &Protection {
                signer: Some(&signer),
                recipients: std::slice::from_ref(&public),
                protect_headers: false,
                gossip: &[],
                now: now(),
            },
        )
        .unwrap();
        let outcome = read_message(&ring, &sent).unwrap();
        let status = outcome.status.unwrap();
        let encryption = status.encryption.unwrap();
        assert_eq!(encryption.state, DecryptionState::Decrypted);
        assert_eq!(encryption.key_id.as_ref(), Some(&env.key));
        assert_eq!(status.signatures[0].state, SignatureState::Valid);
        let display = outcome.display.unwrap();
        assert!(String::from_utf8_lossy(&display).contains("Signed on the card."));
        // One PIN for both operations (PIN cache), two operations.
        assert_eq!(env.pins.requests().len(), 1);
        assert_eq!(env.token.stats().operations, 2);
    }

    #[test]
    fn cancelled_pins_and_missing_cards() {
        let env = Env::new(&[None, None]);
        env.attach_both();
        let ring = env.keyring();
        let stored = env.store.get_key(&env.key).unwrap();
        let public = ring.public_key(&stored).unwrap();
        // Encrypted to the card key; the PIN prompt is cancelled: locked.
        let sent = protect_message(
            PLAIN,
            &Protection {
                signer: None,
                recipients: std::slice::from_ref(&public),
                protect_headers: false,
                gossip: &[],
                now: now(),
            },
        )
        .unwrap();
        let status = read_message(&ring, &sent).unwrap().status.unwrap();
        let encryption = status.encryption.unwrap();
        assert_eq!(encryption.state, DecryptionState::Locked);
        assert_eq!(encryption.key_id.as_ref(), Some(&env.key));
        // Signing with a cancelled PIN reports the card's error.
        let signer = ring.unlocked_key(&stored).unwrap();
        let err = protect_message(
            PLAIN,
            &Protection {
                signer: Some(&signer),
                recipients: &[],
                protect_headers: false,
                gossip: &[],
                now: now(),
            },
        )
        .unwrap_err();
        assert!(matches!(&err, Error::Smartcard(e) if e.kind == SmartcardError::PinCancelled));
        assert_eq!(err.localized().unwrap().id, "smartcard-error-pin-cancelled");
        // The card is gone: unlocking the key says so.
        env.token.set_present(0, false);
        assert!(matches!(
            ring.unlocked_key(&stored),
            Err(Error::Smartcard(e)) if e.kind == SmartcardError::TokenNotPresent
        ));
        let status = read_message(&ring, &sent).unwrap().status.unwrap();
        assert_eq!(status.encryption.unwrap().state, DecryptionState::Locked);
        // Without smartcard support: unsupported.
        let plain = Keyring::new(&env.store, &env.secrets, &env.cache);
        assert!(matches!(
            plain.unlocked_key(&stored),
            Err(Error::Unsupported(_))
        ));
    }

    #[test]
    fn references_accept_one_or_many() {
        let one = reference_of(&[key_ref("01")]);
        assert!(one.starts_with('{'));
        assert_eq!(parse_references(&one).unwrap(), [key_ref("01")]);
        let many = reference_of(&[key_ref("01"), key_ref("02")]);
        assert!(many.starts_with('['));
        assert_eq!(parse_references(&many).unwrap().len(), 2);
        assert!(parse_references("nonsense").is_err());
    }
}
