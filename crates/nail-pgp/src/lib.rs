//! OpenPGP and Autocrypt for Nail (M2a WP31, ADR 0030), built
//! on [`pgp`] (rPGP, MIT / Apache-2.0; not Sequoia, which is LGPL).
//!
//! A library over bytes and the store's keyring; it never talks to the
//! network or the OS by itself (HTTP and the keychain are injected). The
//! core (WP35) wires it into requests, reading and sending.
//!
//! | Module | What |
//! | --- | --- |
//! | [`keyring`] | Keys in `nail_store`'s keyring: import (armored / binary, merging, third-party certifications dropped), generation (Ed25519 + X25519, RSA 4096), export, trust, unlock. Secret keys stay OpenPGP-S2K-protected with the user's passphrase or with a random secret in the OS keychain ([`nail_api::engine::SecretStore`]). |
//! | [`key`] | Parsing and the validity policy: verified self-signatures, expiry, revocation, usable signing / encryption (sub)keys. |
//! | [`compose`] | Outgoing PGP/MIME (RFC 3156): sign, encrypt, sign + encrypt, header protection (`...` subject on the wire). |
//! | [`read`] | Incoming: decrypt / verify PGP/MIME, inline PGP (read-only), header protection; returns [`nail_api::security::SecurityStatus`] and a display message. |
//! | [`autocrypt`] | Autocrypt Level 1: headers, peer state, gossip, recommendation, setup messages. |
//! | [`discovery`] | WKD and keys.openpgp.org lookups through an injected [`discovery::HttpClient`]. |
//!
//! Results use the contract types of [`nail_api::security`]. Interop
//! vectors made with GnuPG 2.4 in Thunderbird's MIME layout are in
//! `crates/nail-testkit/corpus/crypto/pgp/` (see its README).

#![warn(missing_docs)]

pub mod autocrypt;
pub mod compose;
pub mod discovery;
pub mod error;
pub mod key;
pub mod keyring;
mod mime;
pub mod read;
#[cfg(test)]
mod testutil;

pub use error::{Error, Problem, Result};

use nail_api::security::CryptoTechnology;

/// The technology this crate implements.
pub const TECHNOLOGY: CryptoTechnology = CryptoTechnology::OpenPgp;

#[cfg(test)]
mod tests {
    //! Smoke tests: rPGP builds on Windows MSVC and generates, encrypts,
    //! decrypts and serializes (WP30 verifies it before WP31 relies on it).

    use pgp::composed::{
        Deserializable, EncryptionCaps, KeyType, Message, MessageBuilder, SecretKeyParamsBuilder,
        SignedPublicKey, SignedSecretKey, SubkeyParamsBuilder,
    };
    use pgp::crypto::ecc_curve::ECCCurve;
    use pgp::crypto::sym::SymmetricKeyAlgorithm;
    use pgp::types::KeyDetails;
    use rand::thread_rng;

    fn generate(uid: &str) -> SignedSecretKey {
        let mut encryption = SubkeyParamsBuilder::default();
        encryption
            .key_type(KeyType::ECDH(ECCCurve::Curve25519Legacy))
            .can_sign(false)
            .can_encrypt(EncryptionCaps::All)
            .can_authenticate(false);
        let mut params = SecretKeyParamsBuilder::default();
        params
            .key_type(KeyType::Ed25519Legacy)
            .can_certify(true)
            .can_sign(true)
            .can_encrypt(EncryptionCaps::None)
            .primary_user_id(uid.into())
            .subkeys(vec![encryption.build().unwrap()]);
        params.build().unwrap().generate(thread_rng()).unwrap()
    }

    #[test]
    fn technology_is_openpgp() {
        assert_eq!(super::TECHNOLOGY.as_str(), "openpgp");
    }

    #[test]
    fn generate_armor_encrypt_and_decrypt() {
        let secret = generate("Ada Lovelace <ada@example.org>");
        secret.verify_bindings().unwrap();
        let public = SignedPublicKey::from(secret.clone());
        let armored = public.to_armored_string(Default::default()).unwrap();
        assert!(armored.starts_with("-----BEGIN PGP PUBLIC KEY BLOCK-----"));
        let (parsed, _) = SignedPublicKey::from_string(&armored).unwrap();
        assert_eq!(parsed.fingerprint(), secret.fingerprint());

        let subkey = &parsed.public_subkeys[0];
        let mut builder = MessageBuilder::from_bytes("", b"secret".to_vec())
            .seipd_v1(thread_rng(), SymmetricKeyAlgorithm::AES256);
        builder.encrypt_to_key(thread_rng(), &subkey).unwrap();
        let encrypted = builder.to_vec(thread_rng()).unwrap();

        let mut decrypted = Message::from_bytes(&encrypted[..])
            .unwrap()
            .decrypt(&"".into(), &secret)
            .unwrap();
        if decrypted.is_compressed() {
            decrypted = decrypted.decompress().unwrap();
        }
        assert_eq!(decrypted.as_data_vec().unwrap(), b"secret");
    }

    #[test]
    fn wkd_hash_uses_sha1() {
        use sha1::Digest;
        // SHA-1 of the lowercased local part; WKD encodes it in z-base32.
        let digest = sha1::Sha1::digest(b"joe.doe");
        assert_eq!(digest.len(), 20);
    }
}
