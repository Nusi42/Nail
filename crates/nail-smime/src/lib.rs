//! S/MIME for Nail (M2a WP32; stub after WP30, ADR 0029).
//!
//! Expected shape: S/MIME 4.0 (RFC 8551) on top of RustCrypto's [`cms`]
//! and [`x509_cert`] — sign (detached `multipart/signed` and opaque
//! `application/pkcs7-mime; smime-type=signed-data`), encrypt
//! (`enveloped-data` / `authEnveloped-data`: AES-GCM, AES-CBC for
//! compatibility; RSA key transport and ECDH key agreement), decrypt and
//! verify; certificates and private keys from PKCS#12 ([`p12_keystore`])
//! and the Windows certificate store (the `windows` crate, CryptoAPI /
//! CNG, `cfg(windows)` only; the private key never leaves the store);
//! chain building and validation against the Windows trust store, expiry
//! and key-usage checks; revocation reported as "not checked" in M2.
//!
//! Results use the contract types of [`nail_api::security`]
//! (`SecurityStatus`, `SignatureInfo`, `CertificateChain`, `CryptoKey`,
//! `WindowsCertificate`); certificates and (encrypted) private keys are
//! stored through `nail_store`'s keyring (`Store::upsert_key`,
//! `technology = smime`, fingerprint = SHA-256 of the DER certificate).
//!
//! Calling Windows APIs needs `unsafe`: keep it in one module with a
//! module-level `#[allow(unsafe_code)]` and a safety comment per call (the
//! workspace denies `unsafe_code` everywhere else).

#![warn(missing_docs)]

use nail_api::security::CryptoTechnology;

/// The technology this crate implements.
pub const TECHNOLOGY: CryptoTechnology = CryptoTechnology::Smime;

#[cfg(test)]
mod tests {
    //! Smoke tests: the chosen RustCrypto generation builds on Windows MSVC
    //! and its pieces fit together (WP30 verifies them before WP32 relies
    //! on them).

    use std::str::FromStr;
    use std::time::Duration;

    use cms::builder::{SignedDataBuilder, SignerInfoBuilder};
    use cms::cert::{CertificateChoices, IssuerAndSerialNumber};
    use cms::content_info::ContentInfo;
    use cms::signed_data::{EncapsulatedContentInfo, SignedData, SignerIdentifier};
    use der::asn1::OctetString;
    use der::{Any, Decode, Encode, Tag};
    use p256::ecdsa::{DerSignature, SigningKey};
    use rand::rngs::OsRng;
    use spki::{AlgorithmIdentifierOwned, SubjectPublicKeyInfoOwned};
    use x509_cert::builder::{Builder, CertificateBuilder, Profile};
    use x509_cert::name::Name;
    use x509_cert::serial_number::SerialNumber;
    use x509_cert::time::Validity;

    #[test]
    fn technology_is_smime() {
        assert_eq!(super::TECHNOLOGY.as_str(), "smime");
    }

    #[test]
    fn certificate_and_signed_data_round_trip() {
        let key = SigningKey::random(&mut OsRng);
        let spki = SubjectPublicKeyInfoOwned::from_key(*key.verifying_key()).unwrap();
        let subject = Name::from_str("CN=Ada Lovelace,O=Nail Test").unwrap();
        let certificate = CertificateBuilder::new(
            Profile::Root,
            SerialNumber::from(7u32),
            Validity::from_now(Duration::from_secs(3600)).unwrap(),
            subject.clone(),
            spki,
            &key,
        )
        .unwrap()
        .build::<DerSignature>()
        .unwrap();
        let der = certificate.to_der().unwrap();
        let parsed = x509_cert::Certificate::from_der(&der).unwrap();
        assert_eq!(parsed.tbs_certificate.subject, subject);

        let content = EncapsulatedContentInfo {
            econtent_type: const_oid::db::rfc5911::ID_DATA,
            econtent: Some(
                Any::new(
                    Tag::OctetString,
                    OctetString::new(b"hello".to_vec())
                        .unwrap()
                        .to_der()
                        .unwrap(),
                )
                .unwrap(),
            ),
        };
        let digest = AlgorithmIdentifierOwned {
            oid: const_oid::db::rfc5912::ID_SHA_256,
            parameters: None,
        };
        let sid = SignerIdentifier::IssuerAndSerialNumber(IssuerAndSerialNumber {
            issuer: parsed.tbs_certificate.issuer.clone(),
            serial_number: parsed.tbs_certificate.serial_number.clone(),
        });
        let signer = SignerInfoBuilder::new(&key, sid, digest.clone(), &content, None).unwrap();
        let info = SignedDataBuilder::new(&content)
            .add_digest_algorithm(digest)
            .unwrap()
            .add_certificate(CertificateChoices::Certificate(parsed))
            .unwrap()
            .add_signer_info::<SigningKey, DerSignature>(signer)
            .unwrap()
            .build()
            .unwrap();
        let der = info.to_der().unwrap();
        let back = ContentInfo::from_der(&der).unwrap();
        let signed: SignedData = back.content.decode_as().unwrap();
        assert_eq!(signed.signer_infos.0.len(), 1);
    }

    #[test]
    fn content_ciphers_round_trip() {
        use aes_gcm::aead::{Aead, KeyInit};
        let key = aes_gcm::Aes256Gcm::generate_key(&mut OsRng);
        let cipher = aes_gcm::Aes256Gcm::new(&key);
        let nonce = aes_gcm::Nonce::from_slice(&[7u8; 12]);
        let sealed = cipher.encrypt(nonce, b"secret".as_ref()).unwrap();
        assert_eq!(cipher.decrypt(nonce, sealed.as_ref()).unwrap(), b"secret");

        use cbc::cipher::block_padding::Pkcs7;
        use cbc::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit};
        let (key, iv) = ([1u8; 16], [2u8; 16]);
        let encrypted = cbc::Encryptor::<aes::Aes128>::new(&key.into(), &iv.into())
            .encrypt_padded_vec_mut::<Pkcs7>(b"legacy clients");
        let decrypted = cbc::Decryptor::<aes::Aes128>::new(&key.into(), &iv.into())
            .decrypt_padded_vec_mut::<Pkcs7>(&encrypted)
            .unwrap();
        assert_eq!(decrypted, b"legacy clients");

        use sha2_legacy::Digest;
        assert_eq!(sha2_legacy::Sha256::digest(b"").len(), 32);
        assert_eq!(sha1::Sha1::digest(b"").len(), 20);
    }

    /// The `windows` crate links against crypt32 and can open (and walk)
    /// the current user's "Personal" store; nothing is changed.
    #[cfg(windows)]
    #[test]
    #[allow(unsafe_code)]
    fn windows_certificate_store_opens() {
        use windows::Win32::Security::Cryptography::{
            CertCloseStore, CertEnumCertificatesInStore, CertOpenSystemStoreW,
        };
        use windows::core::w;
        // SAFETY: a system store name as a static wide string; the handle is
        // closed below and the enumeration only reads context pointers the
        // API hands back (each call frees the previous one).
        unsafe {
            let store = CertOpenSystemStoreW(None, w!("MY")).unwrap();
            let mut count = 0usize;
            let mut context = CertEnumCertificatesInStore(store, None);
            while !context.is_null() {
                count += 1;
                context = CertEnumCertificatesInStore(store, Some(context));
            }
            let _ = count;
            CertCloseStore(Some(store), 0).unwrap();
        }
    }

    #[test]
    fn pkcs12_reader_rejects_garbage() {
        assert!(p12_keystore::KeyStore::from_pkcs12(b"not a pkcs12 file", "pw").is_err());
    }
}
