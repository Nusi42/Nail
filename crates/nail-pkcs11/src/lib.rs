//! PKCS#11 smartcards for Nail (plan M4c WP122; contract ADR 0094,
//! `nail_api::smartcard`; decisions in ADR 0095).
//!
//! * [`Module`]: what Nail needs from a PKCS#11 module (tokens, objects,
//!   login, sign, decrypt, ECDH). [`CryptokiModule`] implements it over the
//!   vendor DLL with `cryptoki` (Apache-2.0; loaded at run time, no
//!   OpenSSL); the `testing` feature adds [`testing::SoftToken`], an
//!   in-memory token with real RSA / P-256 keys for tests without hardware.
//! * [`Smartcards`]: the configured modules
//!   ([`nail_api::smartcard::SmartcardSettings`]), the token listing as the
//!   contract's [`nail_api::smartcard::SmartcardToken`]s, and
//!   [`CardKey`]s — private keys that sign / decrypt on the card. A
//!   missing login asks for the PIN through a [`PinPrompter`] (the core's
//!   [`nail_api::smartcard::PinSource`] via [`BlockingPinSource`]); wrong
//!   PINs ask again (`retry`, attempts left), a locked PIN or a cancelled
//!   prompt stop. PINs are never stored: the card session stays logged in
//!   for the PIN cache time and is logged out afterwards.
//! * Helpers for the crypto crates: [`digest_info`] (RSA PKCS#1 v1.5
//!   signing input), [`ecdsa_raw_to_der`], [`public_key_of_spki`],
//!   [`certificate_facts`].

#![warn(missing_docs)]

mod cards;
mod cryptoki_module;
mod error;
mod module;
mod pin;
#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use cards::{CardKey, KeyResolver, Smartcards};
pub use cryptoki_module::CryptokiModule;
pub use error::CardError;
pub use module::{
    DecryptMechanism, HashKind, Module, ObjectInfo, PublicKeyMaterial, SignMechanism, TokenInfo,
};
pub use pin::{BlockingPinSource, FixedPin, PinPrompter, block_on};

use nail_api::smartcard::{SmartcardCertificate, SmartcardError};

/// Tries to load a PKCS#11 module: whether the file exists and loads.
pub fn probe_module(path: &str) -> Result<(), SmartcardError> {
    CryptokiModule::load(path).map(|_| ()).map_err(|e| e.kind)
}

/// The DER `DigestInfo` prefix of a hash (RFC 8017 section 9.2, note 1).
fn digest_info_prefix(hash: HashKind) -> &'static [u8] {
    match hash {
        HashKind::Sha1 => &[
            0x30, 0x21, 0x30, 0x09, 0x06, 0x05, 0x2b, 0x0e, 0x03, 0x02, 0x1a, 0x05, 0x00, 0x04,
            0x14,
        ],
        HashKind::Sha256 => &[
            0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02,
            0x01, 0x05, 0x00, 0x04, 0x20,
        ],
        HashKind::Sha384 => &[
            0x30, 0x41, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02,
            0x02, 0x05, 0x00, 0x04, 0x30,
        ],
        HashKind::Sha512 => &[
            0x30, 0x51, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02,
            0x03, 0x05, 0x00, 0x04, 0x40,
        ],
    }
}

/// The input of `CKM_RSA_PKCS` signing: the DER `DigestInfo` of a digest.
pub fn digest_info(hash: HashKind, digest: &[u8]) -> Result<Vec<u8>, CardError> {
    if digest.len() != hash.len() {
        return Err(CardError::new(
            SmartcardError::MechanismUnsupported,
            format!(
                "a {:?} digest has {} bytes, not {}",
                hash,
                hash.len(),
                digest.len()
            ),
        ));
    }
    let mut out = digest_info_prefix(hash).to_vec();
    out.extend_from_slice(digest);
    Ok(out)
}

fn der_integer(bytes: &[u8]) -> Vec<u8> {
    let trimmed = {
        let skip = bytes.iter().take_while(|b| **b == 0).count();
        let rest = &bytes[skip.min(bytes.len().saturating_sub(1))..];
        if rest.is_empty() { &[0u8][..] } else { rest }
    };
    let mut body = Vec::with_capacity(trimmed.len() + 1);
    if trimmed[0] & 0x80 != 0 {
        body.push(0);
    }
    body.extend_from_slice(trimmed);
    let mut out = vec![0x02];
    push_len(&mut out, body.len());
    out.extend(body);
    out
}

fn push_len(out: &mut Vec<u8>, len: usize) {
    if len < 0x80 {
        out.push(len as u8);
    } else if len < 0x100 {
        out.extend([0x81, len as u8]);
    } else {
        out.extend([0x82, (len >> 8) as u8, len as u8]);
    }
}

/// Converts a raw ECDSA signature (`r || s`, as `CKM_ECDSA` returns it)
/// to the DER `Ecdsa-Sig-Value` that CMS and X.509 use.
pub fn ecdsa_raw_to_der(raw: &[u8]) -> Result<Vec<u8>, CardError> {
    if raw.is_empty() || !raw.len().is_multiple_of(2) {
        return Err(CardError::new(
            SmartcardError::DeviceError,
            "the card returned a malformed ECDSA signature",
        ));
    }
    let (r, s) = raw.split_at(raw.len() / 2);
    let body = [der_integer(r), der_integer(s)].concat();
    let mut out = vec![0x30];
    push_len(&mut out, body.len());
    out.extend(body);
    Ok(out)
}

/// The named curve of DER `ECParameters` (an OID).
pub fn curve_name(params: &[u8]) -> Option<&'static str> {
    const P256: &[u8] = &[0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07];
    const P384: &[u8] = &[0x06, 0x05, 0x2b, 0x81, 0x04, 0x00, 0x22];
    const P521: &[u8] = &[0x06, 0x05, 0x2b, 0x81, 0x04, 0x00, 0x23];
    const ED25519: &[u8] = &[0x06, 0x03, 0x2b, 0x65, 0x70];
    const X25519: &[u8] = &[0x06, 0x03, 0x2b, 0x65, 0x6e];
    match params {
        P256 => Some("P-256"),
        P384 => Some("P-384"),
        P521 => Some("P-521"),
        ED25519 => Some("Ed25519"),
        X25519 => Some("X25519"),
        _ => None,
    }
}

/// `CKA_EC_POINT` is a DER OCTET STRING around the SEC1 point; some tokens
/// return the bare point. Returns the bare point either way.
pub fn unwrap_ec_point(value: &[u8]) -> Vec<u8> {
    // Bare uncompressed points of P-256 / P-384 / P-521.
    if matches!(value.len(), 65 | 97 | 133) {
        return value.to_vec();
    }
    if let [0x04, rest @ ..] = value {
        let (len, body) = match rest {
            [l, body @ ..] if *l < 0x80 => (usize::from(*l), body),
            [0x81, l, body @ ..] => (usize::from(*l), body),
            [0x82, h, l, body @ ..] => ((usize::from(*h) << 8) | usize::from(*l), body),
            _ => return value.to_vec(),
        };
        if len == body.len() {
            return body.to_vec();
        }
    }
    value.to_vec()
}

/// The public key of a certificate's `SubjectPublicKeyInfo`.
pub fn public_key_of_spki(
    spki: &x509_cert::spki::SubjectPublicKeyInfoOwned,
) -> Option<PublicKeyMaterial> {
    use der::Encode;
    use rsa::pkcs1::DecodeRsaPublicKey;
    use rsa::traits::PublicKeyParts;
    const RSA: const_oid::ObjectIdentifier = const_oid::db::rfc5912::RSA_ENCRYPTION;
    const EC: const_oid::ObjectIdentifier = const_oid::db::rfc5912::ID_EC_PUBLIC_KEY;
    let bits = spki.subject_public_key.raw_bytes();
    if spki.algorithm.oid == RSA {
        let key = rsa::RsaPublicKey::from_pkcs1_der(bits).ok()?;
        Some(PublicKeyMaterial::Rsa {
            modulus: key.n().to_bytes_be(),
            exponent: key.e().to_bytes_be(),
        })
    } else if spki.algorithm.oid == EC {
        let params = spki.algorithm.parameters.as_ref()?.to_der().ok()?;
        Some(PublicKeyMaterial::Ec {
            params,
            point: bits.to_vec(),
        })
    } else {
        None
    }
}

fn rfc3339(time: x509_cert::time::Time) -> String {
    let secs = time.to_unix_duration().as_secs();
    chrono::DateTime::<chrono::Utc>::from_timestamp(i64::try_from(secs).unwrap_or(i64::MAX), 0)
        .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .unwrap_or_default()
}

/// Facts about a DER certificate on a token (subject, issuer, serial,
/// validity, e-mail addresses, SHA-256 fingerprint).
pub fn certificate_facts(der_bytes: &[u8]) -> Option<SmartcardCertificate> {
    use der::Decode;
    use sha2::Digest;
    use x509_cert::ext::pkix::SubjectAltName;
    use x509_cert::ext::pkix::name::GeneralName;
    let cert = x509_cert::Certificate::from_der(der_bytes).ok()?;
    let tbs = &cert.tbs_certificate;
    let mut emails = Vec::new();
    for ext in tbs.extensions.iter().flatten() {
        if ext.extn_id == const_oid::db::rfc5280::ID_CE_SUBJECT_ALT_NAME
            && let Ok(san) = SubjectAltName::from_der(ext.extn_value.as_bytes())
        {
            for name in san.0 {
                if let GeneralName::Rfc822Name(mail) = name {
                    emails.push(mail.to_string().to_lowercase());
                }
            }
        }
    }
    Some(SmartcardCertificate {
        subject: tbs.subject.to_string(),
        issuer: tbs.issuer.to_string(),
        serial: hex::encode(tbs.serial_number.as_bytes()),
        not_before: rfc3339(tbs.validity.not_before),
        not_after: rfc3339(tbs.validity.not_after),
        emails,
        sha256: hex::encode(sha2::Sha256::digest(der_bytes)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_and_broken_modules_are_refused() {
        assert_eq!(
            probe_module(r"C:\does\not\exist\pkcs11.dll"),
            Err(SmartcardError::ModuleNotFound)
        );
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("fake-pkcs11.dll");
        std::fs::write(&fake, b"not a module").unwrap();
        assert_eq!(
            probe_module(fake.to_str().unwrap()),
            Err(SmartcardError::ModuleLoadFailed)
        );
    }

    #[test]
    fn digest_infos_have_the_right_prefixes_and_lengths() {
        let info = digest_info(HashKind::Sha256, &[7; 32]).unwrap();
        assert_eq!(info.len(), 19 + 32);
        assert_eq!(info[1] as usize, info.len() - 2);
        for hash in [HashKind::Sha1, HashKind::Sha384, HashKind::Sha512] {
            let info = digest_info(hash, &vec![1; hash.len()]).unwrap();
            assert_eq!(info[1] as usize, info.len() - 2, "{hash:?}");
        }
        assert!(digest_info(HashKind::Sha256, &[0; 20]).is_err());
    }

    #[test]
    fn ecdsa_signatures_convert_to_der() {
        // r with the high bit set gets a leading zero; s with leading zeros
        // is trimmed.
        let mut raw = vec![0x80; 32];
        raw.extend(std::iter::repeat_n(0, 31));
        raw.push(5);
        let der = ecdsa_raw_to_der(&raw).unwrap();
        assert_eq!(&der[..4], &[0x30, 2 + 33 + 3, 0x02, 33]);
        assert_eq!(der[4], 0);
        assert_eq!(&der[der.len() - 3..], &[0x02, 1, 5]);
        assert!(ecdsa_raw_to_der(&[1, 2, 3]).is_err());
    }

    #[test]
    fn curves_and_points() {
        assert_eq!(
            curve_name(&[0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07]),
            Some("P-256")
        );
        assert_eq!(curve_name(&[0x06, 0x01, 0x00]), None);
        let point = [0x04u8; 65];
        let mut wrapped = vec![0x04, 65];
        wrapped.extend(point);
        assert_eq!(unwrap_ec_point(&wrapped), point);
        assert_eq!(unwrap_ec_point(&point), point);
    }
}
