//! The operations Nail needs from a PKCS#11 module, as an object-safe
//! trait: [`crate::CryptokiModule`] implements it over a vendor DLL, the
//! `testing` feature's soft token in memory (tests without hardware).

use std::fmt;

use nail_api::smartcard::SmartcardObjectKind;
use zeroize::Zeroizing;

use crate::error::CardError;

/// A token in a slot.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "mirrors independent PKCS#11 token flags (CKF_LOGIN_REQUIRED, CKF_USER_PIN_COUNT_LOW, ...)"
)]
pub struct TokenInfo {
    /// The PKCS#11 slot id.
    pub slot_id: u64,
    /// Token label.
    pub label: String,
    /// Manufacturer.
    pub manufacturer: String,
    /// Model.
    pub model: String,
    /// Serial number (identifies the card in key references).
    pub serial: String,
    /// A PIN is needed for private-key operations.
    pub login_required: bool,
    /// The last PIN attempt failed (`CKF_USER_PIN_COUNT_LOW`).
    pub pin_count_low: bool,
    /// One attempt is left (`CKF_USER_PIN_FINAL_TRY`).
    pub pin_final_try: bool,
    /// The PIN is locked (`CKF_USER_PIN_LOCKED`).
    pub pin_locked: bool,
}

/// Public key material of a key object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublicKeyMaterial {
    /// RSA: big-endian modulus and public exponent.
    Rsa {
        /// `CKA_MODULUS`.
        modulus: Vec<u8>,
        /// `CKA_PUBLIC_EXPONENT`.
        exponent: Vec<u8>,
    },
    /// Elliptic curve: DER `ECParameters` (the curve OID) and the
    /// uncompressed SEC1 point.
    Ec {
        /// `CKA_EC_PARAMS`.
        params: Vec<u8>,
        /// The SEC1 point (the DER OCTET STRING of `CKA_EC_POINT` unwrapped).
        point: Vec<u8>,
    },
}

impl PublicKeyMaterial {
    /// Whether both are the same public key: RSA moduli equal up to
    /// leading zero bytes, EC points equal.
    pub fn same_public_key(&self, other: &Self) -> bool {
        fn trim(bytes: &[u8]) -> &[u8] {
            let skip = bytes.iter().take_while(|b| **b == 0).count();
            &bytes[skip..]
        }
        match (self, other) {
            (Self::Rsa { modulus: a, .. }, Self::Rsa { modulus: b, .. }) => trim(a) == trim(b),
            (Self::Ec { point: a, .. }, Self::Ec { point: b, .. }) => a == b,
            _ => false,
        }
    }

    /// A short algorithm name (`RSA 2048`, `ECC P-256`).
    pub fn algorithm_name(&self) -> String {
        match self {
            Self::Rsa { modulus, .. } => {
                let skip = modulus.iter().take_while(|b| **b == 0).count();
                let bits = modulus.len().saturating_sub(skip) * 8;
                format!("RSA {bits}")
            }
            Self::Ec { params, .. } => match crate::curve_name(params) {
                Some(name) => format!("ECC {name}"),
                None => "ECC".into(),
            },
        }
    }
}

/// An object on a token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectInfo {
    /// Private key, certificate or public key.
    pub kind: SmartcardObjectKind,
    /// `CKA_ID` (pairs a private key with its certificate / public key).
    pub id: Vec<u8>,
    /// `CKA_LABEL`.
    pub label: String,
    /// Public key material (public keys; private keys when the token shows
    /// it).
    pub public_key: Option<PublicKeyMaterial>,
    /// `CKA_SIGN`.
    pub can_sign: bool,
    /// `CKA_DECRYPT` (or `CKA_DERIVE` for EC keys).
    pub can_decrypt: bool,
    /// The DER certificate (`CKA_VALUE` of a certificate).
    pub certificate: Option<Vec<u8>>,
}

/// Hash functions of RSA mechanisms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HashKind {
    /// SHA-1 (reading old messages only).
    Sha1,
    /// SHA-256.
    Sha256,
    /// SHA-384.
    Sha384,
    /// SHA-512.
    Sha512,
}

impl HashKind {
    /// Digest length in bytes.
    pub fn len(self) -> usize {
        match self {
            Self::Sha1 => 20,
            Self::Sha256 => 32,
            Self::Sha384 => 48,
            Self::Sha512 => 64,
        }
    }

    /// Never empty; for clippy's `len_without_is_empty`.
    pub fn is_empty(self) -> bool {
        false
    }
}

/// A signing mechanism; the data is always a digest computed by Nail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignMechanism {
    /// `CKM_RSA_PKCS` over a DER `DigestInfo` ([`crate::digest_info`]).
    RsaPkcs1,
    /// `CKM_RSA_PKCS_PSS` over a digest (MGF1 with the same hash, salt =
    /// digest length).
    RsaPss(HashKind),
    /// `CKM_ECDSA` over a digest; the result is `r || s`.
    Ecdsa,
}

/// A decryption mechanism.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecryptMechanism {
    /// `CKM_RSA_PKCS` (PKCS#1 v1.5 encryption padding).
    RsaPkcs1,
    /// `CKM_RSA_PKCS_OAEP` with this hash and MGF1, no label.
    RsaOaep(HashKind),
}

/// An operation with a private key on a card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyOp<'a> {
    /// Sign `data` (a digest, or a `DigestInfo` for
    /// [`SignMechanism::RsaPkcs1`]).
    Sign {
        /// How.
        mechanism: SignMechanism,
        /// What.
        data: &'a [u8],
    },
    /// Decrypt a key-transport ciphertext.
    Decrypt {
        /// How.
        mechanism: DecryptMechanism,
        /// The ciphertext.
        data: &'a [u8],
    },
    /// ECDH (`CKM_ECDH1_DERIVE`, no KDF) with a peer's SEC1 point: the raw
    /// shared secret.
    Ecdh {
        /// The peer's SEC1 point.
        peer_point: &'a [u8],
    },
}

/// What Nail needs from a loaded PKCS#11 module. Implementations keep one
/// session per slot; `login` logs that session in (the PIN is used once
/// and dropped by the caller).
pub trait Module: Send + Sync + fmt::Debug {
    /// The module path (as configured).
    fn path(&self) -> &str;
    /// The tokens currently present.
    fn tokens(&self) -> Result<Vec<TokenInfo>, CardError>;
    /// The token in a slot (fresh flags: PIN counters).
    fn token(&self, slot_id: u64) -> Result<TokenInfo, CardError>;
    /// The objects visible on a token (without logging in).
    fn objects(&self, slot_id: u64) -> Result<Vec<ObjectInfo>, CardError>;
    /// Whether the slot's session is logged in.
    fn is_logged_in(&self, slot_id: u64) -> bool;
    /// Logs the slot's session in with a user PIN.
    fn login(&self, slot_id: u64, pin: &str) -> Result<(), CardError>;
    /// Logs out (and forgets the session's login state).
    fn logout(&self, slot_id: u64);
    /// Runs `op` with the private key `id`: the signature, the plaintext or
    /// the shared secret. A missing login is `Ok(None)` (never a
    /// [`CardError`] with [`nail_api::smartcard::SmartcardError::PinCancelled`]).
    fn private_op(
        &self,
        slot_id: u64,
        id: &[u8],
        op: KeyOp<'_>,
    ) -> Result<Option<Zeroizing<Vec<u8>>>, CardError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_public_key_ignores_leading_zeros_of_moduli() {
        let rsa = |modulus: Vec<u8>| PublicKeyMaterial::Rsa {
            modulus,
            exponent: vec![1, 0, 1],
        };
        let ec = |point: Vec<u8>| PublicKeyMaterial::Ec {
            params: vec![6],
            point,
        };
        assert!(rsa(vec![0, 0, 7, 8]).same_public_key(&rsa(vec![7, 8])));
        assert!(!rsa(vec![7, 8]).same_public_key(&rsa(vec![7, 9])));
        assert!(ec(vec![4, 1]).same_public_key(&ec(vec![4, 1])));
        assert!(!ec(vec![0, 4, 1]).same_public_key(&ec(vec![4, 1])));
        assert!(!rsa(vec![4, 1]).same_public_key(&ec(vec![4, 1])));
    }
}
