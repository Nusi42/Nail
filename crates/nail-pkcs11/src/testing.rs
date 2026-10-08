//! An in-memory PKCS#11 token for tests without hardware (feature
//! `testing`; ADR 0095 explains why no SoftHSM: it is not installed
//! on the build machines and would add a native test dependency).
//!
//! [`SoftToken`] implements [`Module`] with real RSA and P-256 keys and
//! behaves like a card where it matters for Nail: private-key operations
//! need a login, wrong PINs count down (`pin_count_low`, `pin_final_try`)
//! and lock the PIN after three, tokens can be removed and inserted,
//! unsupported mechanisms fail with `CKR_MECHANISM_INVALID`'s class.

use std::sync::{Mutex, PoisonError};

use nail_api::smartcard::{SmartcardError, SmartcardObjectKind};
use rsa::traits::PublicKeyParts;
use zeroize::Zeroizing;

use crate::error::CardError;
use crate::module::{
    DecryptMechanism, HashKind, KeyOp, Module, ObjectInfo, PublicKeyMaterial, SignMechanism,
    TokenInfo,
};

/// Wrong PINs before the PIN locks.
pub const PIN_TRIES: u32 = 3;

/// The user PIN the tests give their soft tokens.
pub const TEST_PIN: &str = "123456";

/// A private key of the soft token.
#[derive(Clone)]
pub enum SoftKey {
    /// RSA.
    Rsa(Box<rsa::RsaPrivateKey>),
    /// P-256.
    P256(p256::SecretKey),
}

impl std::fmt::Debug for SoftKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Rsa(_) => "SoftKey::Rsa",
            Self::P256(_) => "SoftKey::P256",
        })
    }
}

impl SoftKey {
    fn public(&self) -> PublicKeyMaterial {
        match self {
            Self::Rsa(key) => PublicKeyMaterial::Rsa {
                modulus: key.n().to_bytes_be(),
                exponent: key.e().to_bytes_be(),
            },
            Self::P256(key) => PublicKeyMaterial::Ec {
                params: vec![0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07],
                point: {
                    use p256::elliptic_curve::sec1::ToEncodedPoint;
                    key.public_key().to_encoded_point(false).as_bytes().to_vec()
                },
            },
        }
    }
}

#[derive(Debug, Clone)]
struct SoftObject {
    id: Vec<u8>,
    label: String,
    key: SoftKey,
    certificate: Option<Vec<u8>>,
    /// Whether the private key object shows its public attributes.
    show_public: bool,
}

#[derive(Debug, Clone)]
struct SoftSlot {
    slot_id: u64,
    label: String,
    serial: String,
    pin: String,
    present: bool,
    failures: u32,
    logged_in: bool,
    objects: Vec<SoftObject>,
}

/// Counters of what the token did (assertions in tests).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SoftStats {
    /// Successful logins.
    pub logins: u32,
    /// Logouts.
    pub logouts: u32,
    /// Private-key operations (sign, decrypt, ECDH).
    pub operations: u32,
}

/// An in-memory PKCS#11 module.
#[derive(Debug)]
pub struct SoftToken {
    path: String,
    slots: Mutex<Vec<SoftSlot>>,
    stats: Mutex<SoftStats>,
}

fn not_present() -> CardError {
    CardError::new(SmartcardError::TokenNotPresent, "CKR_TOKEN_NOT_PRESENT")
}

impl SoftToken {
    /// An empty module with this path.
    pub fn new(path: &str) -> Self {
        Self {
            path: path.to_owned(),
            slots: Mutex::new(Vec::new()),
            stats: Mutex::new(SoftStats::default()),
        }
    }

    fn slots(&self) -> std::sync::MutexGuard<'_, Vec<SoftSlot>> {
        self.slots.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn bump(&self, f: impl FnOnce(&mut SoftStats)) {
        f(&mut self.stats.lock().unwrap_or_else(PoisonError::into_inner));
    }

    /// What happened so far.
    pub fn stats(&self) -> SoftStats {
        *self.stats.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Adds an inserted token.
    pub fn add_token(&self, slot_id: u64, label: &str, serial: &str, pin: &str) {
        self.slots().push(SoftSlot {
            slot_id,
            label: label.to_owned(),
            serial: serial.to_owned(),
            pin: pin.to_owned(),
            present: true,
            failures: 0,
            logged_in: false,
            objects: Vec::new(),
        });
    }

    /// Adds a private key (with an optional DER certificate under the same
    /// `CKA_ID`). `show_public` = the private key object exposes its
    /// modulus / point (many cards only show it on the public key or the
    /// certificate).
    ///
    /// # Panics
    ///
    /// When no token was added for `slot_id` (a mistake in the test).
    pub fn add_key(
        &self,
        slot_id: u64,
        id: &[u8],
        label: &str,
        key: SoftKey,
        certificate: Option<Vec<u8>>,
        show_public: bool,
    ) {
        let mut slots = self.slots();
        let slot = slots
            .iter_mut()
            .find(|s| s.slot_id == slot_id)
            .unwrap_or_else(|| panic!("no token in slot {slot_id}: add the token first"));
        slot.objects.push(SoftObject {
            id: id.to_vec(),
            label: label.to_owned(),
            key,
            certificate,
            show_public,
        });
    }

    /// Removes (`false`) or inserts (`true`) a token; removal ends its
    /// login.
    pub fn set_present(&self, slot_id: u64, present: bool) {
        for slot in self.slots().iter_mut().filter(|s| s.slot_id == slot_id) {
            slot.present = present;
            slot.logged_in = false;
        }
    }

    /// Moves a token to another slot (a different reader).
    pub fn move_token(&self, from: u64, to: u64) {
        for slot in self.slots().iter_mut().filter(|s| s.slot_id == from) {
            slot.slot_id = to;
            slot.logged_in = false;
        }
    }

    fn with_slot<T>(
        &self,
        slot_id: u64,
        f: impl FnOnce(&mut SoftSlot) -> Result<T, CardError>,
    ) -> Result<T, CardError> {
        let mut slots = self.slots();
        let slot = slots
            .iter_mut()
            .find(|s| s.slot_id == slot_id && s.present)
            .ok_or_else(not_present)?;
        f(slot)
    }

    fn with_key<T>(
        &self,
        slot_id: u64,
        id: &[u8],
        f: impl FnOnce(&SoftKey) -> Result<T, CardError>,
    ) -> Result<Option<T>, CardError> {
        let out = self.with_slot(slot_id, |slot| {
            let object = slot.objects.iter().find(|o| o.id == id).ok_or_else(|| {
                CardError::new(SmartcardError::KeyNotFound, "CKR_KEY_HANDLE_INVALID")
            })?;
            if !slot.logged_in {
                return Ok(None);
            }
            f(&object.key).map(Some)
        })?;
        if out.is_some() {
            self.bump(|s| s.operations += 1);
        }
        Ok(out)
    }
}

fn unsupported() -> CardError {
    CardError::new(
        SmartcardError::MechanismUnsupported,
        "CKR_MECHANISM_INVALID",
    )
}

fn failed(err: impl std::fmt::Display) -> CardError {
    CardError::new(SmartcardError::DeviceError, err.to_string())
}

fn oaep(hash: HashKind) -> rsa::Oaep {
    match hash {
        HashKind::Sha1 => rsa::Oaep::new::<sha1::Sha1>(),
        HashKind::Sha256 => rsa::Oaep::new::<sha2_legacy::Sha256>(),
        HashKind::Sha384 => rsa::Oaep::new::<sha2_legacy::Sha384>(),
        HashKind::Sha512 => rsa::Oaep::new::<sha2_legacy::Sha512>(),
    }
}

impl Module for SoftToken {
    fn path(&self) -> &str {
        &self.path
    }

    fn tokens(&self) -> Result<Vec<TokenInfo>, CardError> {
        let ids: Vec<u64> = self
            .slots()
            .iter()
            .filter(|s| s.present)
            .map(|s| s.slot_id)
            .collect();
        ids.into_iter().map(|id| self.token(id)).collect()
    }

    fn token(&self, slot_id: u64) -> Result<TokenInfo, CardError> {
        self.with_slot(slot_id, |slot| {
            Ok(TokenInfo {
                slot_id,
                label: slot.label.clone(),
                manufacturer: "Nail tests".into(),
                model: "SoftToken".into(),
                serial: slot.serial.clone(),
                login_required: true,
                pin_count_low: slot.failures > 0,
                pin_final_try: slot.failures + 1 == PIN_TRIES,
                pin_locked: slot.failures >= PIN_TRIES,
            })
        })
    }

    fn objects(&self, slot_id: u64) -> Result<Vec<ObjectInfo>, CardError> {
        self.with_slot(slot_id, |slot| {
            let mut out = Vec::new();
            for object in &slot.objects {
                let (sign, decrypt) = (true, true);
                out.push(ObjectInfo {
                    kind: SmartcardObjectKind::PrivateKey,
                    id: object.id.clone(),
                    label: object.label.clone(),
                    public_key: object.show_public.then(|| object.key.public()),
                    can_sign: sign,
                    can_decrypt: decrypt,
                    certificate: None,
                });
                out.push(ObjectInfo {
                    kind: SmartcardObjectKind::PublicKey,
                    id: object.id.clone(),
                    label: object.label.clone(),
                    public_key: Some(object.key.public()),
                    can_sign: false,
                    can_decrypt: false,
                    certificate: None,
                });
                if let Some(cert) = &object.certificate {
                    out.push(ObjectInfo {
                        kind: SmartcardObjectKind::Certificate,
                        id: object.id.clone(),
                        label: object.label.clone(),
                        public_key: None,
                        can_sign: false,
                        can_decrypt: false,
                        certificate: Some(cert.clone()),
                    });
                }
            }
            Ok(out)
        })
    }

    fn is_logged_in(&self, slot_id: u64) -> bool {
        self.with_slot(slot_id, |slot| Ok(slot.logged_in))
            .unwrap_or(false)
    }

    fn login(&self, slot_id: u64, pin: &str) -> Result<(), CardError> {
        self.with_slot(slot_id, |slot| {
            if slot.failures >= PIN_TRIES {
                return Err(CardError::new(SmartcardError::PinLocked, "CKR_PIN_LOCKED"));
            }
            if pin != slot.pin {
                slot.failures += 1;
                return Err(if slot.failures >= PIN_TRIES {
                    CardError::new(SmartcardError::PinLocked, "CKR_PIN_LOCKED")
                } else {
                    CardError::new(SmartcardError::PinIncorrect, "CKR_PIN_INCORRECT")
                });
            }
            slot.failures = 0;
            slot.logged_in = true;
            Ok(())
        })?;
        self.bump(|s| s.logins += 1);
        Ok(())
    }

    fn logout(&self, slot_id: u64) {
        let was = self
            .with_slot(slot_id, |slot| {
                Ok(std::mem::replace(&mut slot.logged_in, false))
            })
            .unwrap_or(false);
        if was {
            self.bump(|s| s.logouts += 1);
        }
    }

    fn private_op(
        &self,
        slot_id: u64,
        id: &[u8],
        op: KeyOp<'_>,
    ) -> Result<Option<Zeroizing<Vec<u8>>>, CardError> {
        self.with_key(slot_id, id, |key| match op {
            KeyOp::Sign { mechanism, data } => sign(key, mechanism, data).map(Zeroizing::new),
            KeyOp::Decrypt { mechanism, data } => decrypt(key, mechanism, data),
            KeyOp::Ecdh { peer_point } => ecdh(key, peer_point),
        })
    }
}

/// Signs with a soft key.
fn sign(key: &SoftKey, mechanism: SignMechanism, data: &[u8]) -> Result<Vec<u8>, CardError> {
    match (key, mechanism) {
        (SoftKey::Rsa(key), SignMechanism::RsaPkcs1) => key
            .sign(rsa::Pkcs1v15Sign::new_unprefixed(), data)
            .map_err(failed),
        (SoftKey::Rsa(key), SignMechanism::RsaPss(hash)) => {
            let mut rng = rand::rngs::OsRng;
            let salt = hash.len();
            let scheme = match hash {
                HashKind::Sha1 => rsa::Pss::new_with_salt::<sha1::Sha1>(salt),
                HashKind::Sha256 => rsa::Pss::new_with_salt::<sha2_legacy::Sha256>(salt),
                HashKind::Sha384 => rsa::Pss::new_with_salt::<sha2_legacy::Sha384>(salt),
                HashKind::Sha512 => rsa::Pss::new_with_salt::<sha2_legacy::Sha512>(salt),
            };
            key.sign_with_rng(&mut rng, scheme, data).map_err(failed)
        }
        (SoftKey::P256(key), SignMechanism::Ecdsa) => {
            use p256::ecdsa::signature::hazmat::PrehashSigner;
            let signer = p256::ecdsa::SigningKey::from(key);
            let sig: p256::ecdsa::Signature = signer.sign_prehash(data).map_err(failed)?;
            Ok(sig.to_bytes().to_vec())
        }
        _ => Err(unsupported()),
    }
}

/// Decrypts with a soft key.
fn decrypt(
    key: &SoftKey,
    mechanism: DecryptMechanism,
    data: &[u8],
) -> Result<Zeroizing<Vec<u8>>, CardError> {
    let SoftKey::Rsa(key) = key else {
        return Err(unsupported());
    };
    let plain = match mechanism {
        DecryptMechanism::RsaPkcs1 => key.decrypt(rsa::Pkcs1v15Encrypt, data),
        DecryptMechanism::RsaOaep(hash) => key.decrypt(oaep(hash), data),
    }
    .map_err(|_| CardError::new(SmartcardError::DeviceError, "CKR_ENCRYPTED_DATA_INVALID"))?;
    Ok(Zeroizing::new(plain))
}

/// ECDH with a soft P-256 key.
fn ecdh(key: &SoftKey, peer_point: &[u8]) -> Result<Zeroizing<Vec<u8>>, CardError> {
    let SoftKey::P256(key) = key else {
        return Err(unsupported());
    };
    let peer = p256::PublicKey::from_sec1_bytes(peer_point)
        .map_err(|_| CardError::new(SmartcardError::DeviceError, "CKR_DOMAIN_PARAMS_INVALID"))?;
    let shared = p256::ecdh::diffie_hellman(key.to_nonzero_scalar(), peer.as_affine());
    Ok(Zeroizing::new(shared.raw_secret_bytes().to_vec()))
}

/// A small RSA key for tests (2048 bits: what the crypto crates accept; never
/// use outside tests).
///
/// # Panics
///
/// When the operating system's random number generator fails.
pub fn test_rsa_key() -> rsa::RsaPrivateKey {
    rsa::RsaPrivateKey::new(&mut rand::rngs::OsRng, 2048)
        .unwrap_or_else(|e| panic!("RSA key generation failed: {e}"))
}

/// A P-256 key for tests.
pub fn test_p256_key() -> p256::SecretKey {
    p256::SecretKey::random(&mut rand::rngs::OsRng)
}
