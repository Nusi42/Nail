//! OpenPGP for Nail (M2a WP31; stub after WP30, ADR 0029).
//!
//! Expected shape: [`pgp`] (rPGP, MIT / Apache-2.0) for
//!
//! * the keyring: public keys and own secret keys, stored through
//!   `nail_store`'s keyring (`Store::upsert_key`, `technology = openpgp`,
//!   fingerprint = primary key fingerprint). Secret keys are stored
//!   encrypted: OpenPGP S2K with the user's passphrase, or with a random
//!   key kept in the OS keychain (`nail_api::engine::SecretStore`);
//! * generation (Ed25519 + X25519 by default, RSA 4096 on request),
//!   import (armored / binary, merging) and export of public keys;
//! * PGP/MIME (RFC 3156) sign, encrypt, sign + encrypt, decrypt and
//!   verify; inline PGP read-only; header protection for encrypted mail
//!   (the subject shown as `...` on the wire);
//! * Autocrypt Level 1: `Autocrypt` / `Autocrypt-Gossip` headers, peer
//!   state (`Store::put_autocrypt_peer`), the encryption recommendation,
//!   setup messages;
//! * key discovery via WKD and keys.openpgp.org behind an injectable HTTP
//!   client (tests use `nail-testkit-net`'s mock HTTP server);
//! * the trust model of [`nail_api::security::TrustLevel`].
//!
//! Results use the contract types of [`nail_api::security`]. Test vectors
//! (keys and messages produced by GnuPG and Thunderbird) go into
//! `crates/nail-testkit/corpus/crypto/pgp/` (`nail_testkit::crypto_fixture`).

#![warn(missing_docs)]

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
