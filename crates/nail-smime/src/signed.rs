//! CMS `SignedData` (RFC 5652 section 5): creating and verifying S/MIME
//! signatures.
//!
//! Nail signs with signed attributes (content type, signing time, message
//! digest, S/MIME capabilities; RFC 8551 section 2.5), SHA-256 (SHA-384
//! for P-384 keys), RSASSA-PKCS1-v1_5 or ECDSA, and includes the signer's
//! certificate and chain. Verification accepts RSA PKCS#1 v1.5, RSA-PSS and
//! ECDSA on P-256 / P-384 with SHA-2; SHA-1 signatures are reported as
//! unsupported, never as valid.

use cms::cert::CertificateChoices;
use cms::content_info::{CmsVersion, ContentInfo};
use cms::signed_data::{
    CertificateSet, EncapsulatedContentInfo, SignedData, SignerIdentifier, SignerInfo, SignerInfos,
};
use der::asn1::{OctetString, SetOfVec};
use der::{Any, Decode, Encode};
use nail_api::security::{SignatureInfo, SignatureState, TrustLevel};
use spki::AlgorithmIdentifierOwned;
use x509_cert::attr::Attribute;
use x509_cert::time::Time;

use crate::ber;
use crate::cert::{Cert, Purpose};
use crate::chain::{self, TrustAnchors};
use crate::error::{Error, Result};
use crate::keys::{HashAlg, PrivateKeyOps};
use crate::oids;
use nail_util::time::rfc3339_utc as rfc3339;

/// Tolerated clock skew for a signing time in the future (one day).
const FUTURE_SKEW: i64 = 86_400;

/// A signer: private key, certificate and the chain to include.
#[derive(Debug, Clone, Copy)]
pub struct Signer<'a> {
    /// The private key.
    pub key: &'a dyn PrivateKeyOps,
    /// Its certificate.
    pub cert: &'a Cert,
    /// Intermediate certificates to include (not the root).
    pub chain: &'a [Cert],
}

/// The content ciphers Nail announces in `SMIMECapabilities`, preferred
/// first.
pub(crate) fn capabilities() -> Vec<AlgorithmIdentifierOwned> {
    [
        oids::AES256_GCM,
        oids::AES128_GCM,
        oids::AES256_CBC,
        oids::AES128_CBC,
    ]
    .into_iter()
    .map(|oid| AlgorithmIdentifierOwned {
        oid,
        parameters: None,
    })
    .collect()
}

fn attribute(oid: const_oid::ObjectIdentifier, value: Any) -> Result<Attribute> {
    Ok(Attribute {
        oid,
        values: SetOfVec::try_from(vec![value])?,
    })
}

/// `signingTime`: `UTCTime` through 2049, `GeneralizedTime` after (RFC 5652
/// section 11.3).
fn signing_time_value(unix: i64) -> Result<Time> {
    let since_epoch = std::time::Duration::from_secs(u64::try_from(unix).unwrap_or(0));
    Ok(match der::asn1::UtcTime::from_unix_duration(since_epoch) {
        Ok(utc) => Time::UtcTime(utc),
        Err(_) => Time::GeneralTime(der::asn1::GeneralizedTime::from_unix_duration(since_epoch)?),
    })
}

/// The DER `SignedData` `ContentInfo` over `content`. `detached` leaves the
/// content out (for `multipart/signed`). `signing_time` is Unix seconds.
pub fn sign(
    content: &[u8],
    signer: &Signer<'_>,
    detached: bool,
    signing_time: i64,
) -> Result<Vec<u8>> {
    let public = signer.cert.public_key()?;
    let scheme = public.default_scheme();
    let hash = scheme.hash();

    let time = signing_time_value(signing_time)?;
    let signed_attrs = SetOfVec::try_from(vec![
        attribute(oids::ID_CONTENT_TYPE, Any::encode_from(&oids::ID_DATA)?)?,
        attribute(oids::ID_SIGNING_TIME, Any::encode_from(&time)?)?,
        attribute(
            oids::ID_MESSAGE_DIGEST,
            Any::encode_from(&OctetString::new(hash.digest(content))?)?,
        )?,
        attribute(
            oids::ID_SMIME_CAPABILITIES,
            Any::encode_from(&capabilities())?,
        )?,
    ])?;
    let to_sign = signed_attrs.to_der()?;
    let digest = hash.digest(&to_sign);
    let signature = signer.key.sign_digest(scheme, &digest)?;
    // Catch a key that does not belong to the certificate (or a faulty
    // key provider) before anything leaves.
    public
        .verify_digest(scheme, &digest, &signature)
        .map_err(|_| Error::Crypto("the private key does not match the certificate".into()))?;

    let signer_info = SignerInfo {
        version: CmsVersion::V1,
        sid: SignerIdentifier::IssuerAndSerialNumber(signer.cert.issuer_and_serial()),
        digest_alg: hash.algorithm_identifier(),
        signed_attrs: Some(signed_attrs),
        signature_algorithm: scheme.cms_algorithm()?,
        signature: OctetString::new(signature)?,
        unsigned_attrs: None,
    };
    let mut certs = vec![CertificateChoices::Certificate(signer.cert.inner().clone())];
    for cert in signer.chain {
        if cert != signer.cert {
            certs.push(CertificateChoices::Certificate(cert.inner().clone()));
        }
    }
    let econtent = if detached {
        None
    } else {
        Some(Any::encode_from(&OctetString::new(content)?)?)
    };
    let signed_data = SignedData {
        version: CmsVersion::V1,
        digest_algorithms: SetOfVec::try_from(vec![hash.algorithm_identifier()])?,
        encap_content_info: EncapsulatedContentInfo {
            econtent_type: oids::ID_DATA,
            econtent,
        },
        certificates: Some(CertificateSet(SetOfVec::try_from(certs)?)),
        crls: None,
        signer_infos: SignerInfos(SetOfVec::try_from(vec![signer_info])?),
    };
    Ok(ContentInfo {
        content_type: oids::ID_SIGNED_DATA,
        content: Any::encode_from(&signed_data)?,
    }
    .to_der()?)
}

/// What a verification needs besides the signature.
#[derive(Clone, Copy)]
pub struct VerifyContext<'a> {
    /// Trust anchors for the chain.
    pub anchors: &'a dyn TrustAnchors,
    /// Certificates known beyond the message (keyring: signers seen
    /// before, intermediates).
    pub known_certs: &'a [Cert],
    /// The message's `From` address, if known.
    pub from: Option<&'a str>,
    /// Now (Unix seconds).
    pub now: i64,
}

impl std::fmt::Debug for VerifyContext<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VerifyContext")
            .field("known_certs", &self.known_certs)
            .field("from", &self.from)
            .field("now", &self.now)
            .finish_non_exhaustive()
    }
}

/// One checked signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignerOutcome {
    /// The protocol result (`key_id` / `trust` are filled by the keyring).
    pub info: SignatureInfo,
    /// The signer's certificate, when it was found.
    pub cert: Option<Cert>,
}

/// A verified `SignedData`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verified {
    /// One entry per `SignerInfo` (empty for a certs-only bundle).
    pub signers: Vec<SignerOutcome>,
    /// The signed content: the encapsulated content, or the detached
    /// content that was passed in.
    pub content: Vec<u8>,
    /// The certificates the message carried.
    pub certificates: Vec<Cert>,
}

/// Verifies a `SignedData` `ContentInfo` (BER or DER). `detached` is the
/// signed content of a `multipart/signed` message. Structural errors are
/// `Err`; per-signature problems are reported in the outcome.
pub fn verify(input: &[u8], detached: Option<&[u8]>, ctx: &VerifyContext<'_>) -> Result<Verified> {
    let der = ber::normalize(input)?;
    let info = ContentInfo::from_der(&der)?;
    if info.content_type != oids::ID_SIGNED_DATA {
        return Err(Error::malformed("not a CMS SignedData"));
    }
    let signed: SignedData = info.content.decode_as()?;
    let content = match (detached, &signed.encap_content_info.econtent) {
        (Some(content), _) => content.to_vec(),
        (None, Some(econtent)) => econtent.decode_as::<OctetString>()?.into_bytes(),
        (None, None) => Vec::new(),
    };
    let mut certificates = Vec::new();
    for choice in signed.certificates.iter().flat_map(|s| s.0.iter()) {
        if let CertificateChoices::Certificate(cert) = choice {
            certificates.push(Cert::from_certificate(cert.clone())?);
        }
    }
    let mut pool = certificates.clone();
    pool.extend(ctx.known_certs.iter().cloned());

    let mut signers = Vec::new();
    for raw in raw_signer_infos(&der)? {
        let signer_info = SignerInfo::from_der(raw.full)?;
        signers.push(verify_signer(
            &signer_info,
            raw.value,
            &content,
            signed.encap_content_info.econtent_type,
            &pool,
            ctx,
        ));
    }
    Ok(Verified {
        signers,
        content,
        certificates,
    })
}

/// The raw `SignerInfo` elements, in message order (the decoded SET is
/// re-sorted, and the signed attributes must be hashed as sent).
fn raw_signer_infos(der: &[u8]) -> Result<Vec<ber::Tlv<'_>>> {
    let (content_info, _) = ber::tlv(der)?;
    let parts = ber::children(content_info.value)?;
    let explicit = parts
        .get(1)
        .ok_or_else(|| Error::malformed("ContentInfo without content"))?;
    let (signed_data, _) = ber::tlv(explicit.value)?;
    let fields = ber::children(signed_data.value)?;
    let signer_infos = fields
        .last()
        .filter(|f| f.tag == 0x31)
        .ok_or_else(|| Error::malformed("SignedData without signerInfos"))?;
    ber::children(signer_infos.value)
}

/// The raw signed attributes of a raw `SignerInfo` value, re-tagged as the
/// SET the signature covers (RFC 5652 section 5.4).
fn raw_signed_attrs(signer_info_value: &[u8]) -> Result<Option<Vec<u8>>> {
    for field in ber::children(signer_info_value)? {
        if field.tag == 0xa0 {
            let mut set = field.full.to_vec();
            set[0] = 0x31;
            return Ok(Some(set));
        }
    }
    Ok(None)
}

fn find_signer(sid: &SignerIdentifier, pool: &[Cert]) -> Option<Cert> {
    pool.iter()
        .find(|c| match sid {
            SignerIdentifier::IssuerAndSerialNumber(id) => c.matches_issuer_serial(id),
            SignerIdentifier::SubjectKeyIdentifier(ski) => c.matches_key_id(ski.0.as_bytes()),
        })
        .cloned()
}

fn attr_value(attrs: &[Attribute], oid: const_oid::ObjectIdentifier) -> Option<&Any> {
    attrs
        .iter()
        .find(|a| a.oid == oid)
        .and_then(|a| a.values.iter().next())
}

fn verify_signer(
    signer_info: &SignerInfo,
    raw_value: &[u8],
    content: &[u8],
    econtent_type: const_oid::ObjectIdentifier,
    pool: &[Cert],
    ctx: &VerifyContext<'_>,
) -> SignerOutcome {
    let cert = find_signer(&signer_info.sid, pool);
    let mut info = SignatureInfo {
        state: SignatureState::Error,
        signer: None,
        signer_matches_from: false,
        key_id: None,
        fingerprint: None,
        trust: TrustLevel::Unknown,
        signed_at: None,
        chain: None,
    };
    let signing_time = signer_info.signed_attrs.as_ref().and_then(|attrs| {
        attr_value(attrs.as_slice(), oids::ID_SIGNING_TIME)
            .and_then(|v| v.to_der().ok())
            .and_then(|der| Time::from_der(&der).ok())
            .map(|t| i64::try_from(t.to_unix_duration().as_secs()).unwrap_or(i64::MAX))
    });
    info.signed_at = signing_time.map(rfc3339);
    if let Some(cert) = &cert {
        info.fingerprint = Some(cert.fingerprint().to_owned());
        info.signer = cert
            .emails()
            .into_iter()
            .next()
            .or_else(|| Some(cert.subject()));
        info.signer_matches_from = ctx.from.is_some_and(|from| cert.has_email(from));
    }
    info.state = match check_signature(
        signer_info,
        raw_value,
        content,
        econtent_type,
        cert.as_ref(),
    ) {
        Ok(()) => SignatureState::Valid,
        Err(state) => state,
    };
    if let Some(cert) = &cert {
        // The certificate is judged at the signing time when the signature
        // names a plausible one, else now.
        let at = signing_time
            .filter(|t| *t <= ctx.now + FUTURE_SKEW)
            .unwrap_or(ctx.now);
        let validation = chain::validate(cert, pool, ctx.anchors, Purpose::Sign, at);
        if info.state == SignatureState::Valid && !cert.valid_at(at) {
            info.state = SignatureState::KeyExpired;
        }
        info.chain = Some(validation.chain);
    }
    SignerOutcome { info, cert }
}

fn check_signature(
    signer_info: &SignerInfo,
    raw_value: &[u8],
    content: &[u8],
    econtent_type: const_oid::ObjectIdentifier,
    cert: Option<&Cert>,
) -> std::result::Result<(), SignatureState> {
    let Some(cert) = cert else {
        return Err(SignatureState::UnknownKey);
    };
    let unsupported = |e: Error| match e {
        Error::Unsupported(_) => SignatureState::Unsupported,
        _ => SignatureState::Error,
    };
    let hash = HashAlg::from_oid(&signer_info.digest_alg.oid).ok_or(SignatureState::Unsupported)?;
    let scheme = crate::keys::SignScheme::resolve(&signer_info.signature_algorithm, Some(hash))
        .map_err(unsupported)?;
    let public = cert.public_key().map_err(unsupported)?;
    let signed_bytes = if let Some(attrs) = &signer_info.signed_attrs {
        let attrs = attrs.as_slice();
        let content_type = attr_value(attrs, oids::ID_CONTENT_TYPE)
            .and_then(|v| v.decode_as::<const_oid::ObjectIdentifier>().ok());
        if content_type != Some(econtent_type) {
            return Err(SignatureState::Invalid);
        }
        let digest = attr_value(attrs, oids::ID_MESSAGE_DIGEST)
            .and_then(|v| v.decode_as::<OctetString>().ok())
            .ok_or(SignatureState::Invalid)?;
        if digest.as_bytes() != hash.digest(content).as_slice() {
            return Err(SignatureState::Invalid);
        }
        raw_signed_attrs(raw_value)
            .ok()
            .flatten()
            .ok_or(SignatureState::Error)?
    } else {
        if econtent_type != oids::ID_DATA {
            return Err(SignatureState::Invalid);
        }
        content.to_vec()
    };
    public
        .verify(scheme, &signed_bytes, signer_info.signature.as_bytes())
        .map_err(|_| SignatureState::Invalid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{AT, fixture_cert, fixture_key, test_anchors};
    use nail_api::security::ChainState;

    fn ctx<'a>(anchors: &'a dyn TrustAnchors, known: &'a [Cert]) -> VerifyContext<'a> {
        VerifyContext {
            anchors,
            known_certs: known,
            from: Some("alice@example.org"),
            now: AT,
        }
    }

    fn alice_signed(detached: bool) -> Vec<u8> {
        let key = fixture_key("alice.key");
        let cert = fixture_cert("alice.crt");
        let chain = [fixture_cert("intermediate-ca.crt")];
        let signer = Signer {
            key: &key,
            cert: &cert,
            chain: &chain,
        };
        sign(b"Hello\r\n", &signer, detached, AT - 60).unwrap()
    }

    #[test]
    fn opaque_round_trip_is_valid_with_chain() {
        let der = alice_signed(false);
        let anchors = test_anchors();
        let verified = verify(&der, None, &ctx(&anchors, &[])).unwrap();
        assert_eq!(verified.content, b"Hello\r\n");
        assert_eq!(verified.certificates.len(), 2);
        let [signer] = verified.signers.as_slice() else {
            panic!("one signer")
        };
        assert_eq!(signer.info.state, SignatureState::Valid);
        assert_eq!(signer.info.signer.as_deref(), Some("alice@example.org"));
        assert!(signer.info.signer_matches_from);
        assert_eq!(
            signer.info.signed_at.as_deref(),
            Some(rfc3339(AT - 60).as_str())
        );
        assert_eq!(
            signer.info.fingerprint.as_deref(),
            Some(fixture_cert("alice.crt").fingerprint())
        );
        assert_eq!(signer.info.chain.as_ref().unwrap().state, ChainState::Valid);
    }

    #[test]
    fn detached_signature_checks_the_content() {
        let der = alice_signed(true);
        let anchors = test_anchors();
        let ok = verify(&der, Some(b"Hello\r\n"), &ctx(&anchors, &[])).unwrap();
        assert_eq!(ok.signers[0].info.state, SignatureState::Valid);
        let bad = verify(&der, Some(b"Hello!\r\n"), &ctx(&anchors, &[])).unwrap();
        assert_eq!(bad.signers[0].info.state, SignatureState::Invalid);
    }

    #[test]
    fn ecdsa_p256_signer() {
        let key = fixture_key("bob.key");
        let cert = fixture_cert("bob.crt");
        let signer = Signer {
            key: &key,
            cert: &cert,
            chain: &[],
        };
        let der = sign(b"x", &signer, false, AT).unwrap();
        let anchors = test_anchors();
        // The chain is completed from known certificates.
        let known = [fixture_cert("intermediate-ca.crt")];
        let verified = verify(&der, None, &ctx(&anchors, &known)).unwrap();
        let info = &verified.signers[0].info;
        assert_eq!(info.state, SignatureState::Valid);
        assert!(!info.signer_matches_from); // bob signed, From says alice
        assert_eq!(info.chain.as_ref().unwrap().state, ChainState::Valid);
    }

    #[test]
    fn a_key_that_does_not_match_the_certificate_cannot_sign() {
        let key = fixture_key("carol.key");
        let cert = fixture_cert("alice.crt");
        let signer = Signer {
            key: &key,
            cert: &cert,
            chain: &[],
        };
        assert!(matches!(
            sign(b"x", &signer, false, AT),
            Err(Error::Crypto(_))
        ));
    }

    #[test]
    fn tampered_signature_bytes_are_invalid() {
        let der = alice_signed(false);
        let anchors = test_anchors();
        // Change the last byte of the signature value (the end of the DER).
        let mut tampered = der.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 0x01;
        let verified = verify(&tampered, None, &ctx(&anchors, &[])).unwrap();
        assert_eq!(verified.signers[0].info.state, SignatureState::Invalid);
    }

    #[test]
    fn tampered_encapsulated_content_is_invalid() {
        let der = alice_signed(false);
        let pos = der.windows(5).position(|w| w == b"Hello").unwrap();
        let mut tampered = der.clone();
        tampered[pos] = b'J';
        let anchors = test_anchors();
        let verified = verify(&tampered, None, &ctx(&anchors, &[])).unwrap();
        assert_eq!(verified.content, b"Jello\r\n");
        assert_eq!(verified.signers[0].info.state, SignatureState::Invalid);
    }

    #[test]
    fn unknown_signer_certificate() {
        // Strip certificates: build a signature, then verify with a pool
        // that lacks alice by signing with a chain but re-encoding without
        // the certificate set.
        let der = alice_signed(false);
        let info = ContentInfo::from_der(&der).unwrap();
        let mut signed: SignedData = info.content.decode_as().unwrap();
        signed.certificates = None;
        let stripped = ContentInfo {
            content_type: oids::ID_SIGNED_DATA,
            content: Any::encode_from(&signed).unwrap(),
        }
        .to_der()
        .unwrap();
        let anchors = test_anchors();
        let verified = verify(&stripped, None, &ctx(&anchors, &[])).unwrap();
        let info = &verified.signers[0].info;
        assert_eq!(info.state, SignatureState::UnknownKey);
        assert!(info.signer.is_none() && info.chain.is_none());
        // Known from the keyring: valid again.
        let known = [
            fixture_cert("alice.crt"),
            fixture_cert("intermediate-ca.crt"),
        ];
        let verified = verify(&stripped, None, &ctx(&anchors, &known)).unwrap();
        assert_eq!(verified.signers[0].info.state, SignatureState::Valid);
    }

    #[test]
    fn expired_signer_at_signing_time() {
        let key = fixture_key("carol.key");
        let cert = fixture_cert("carol.crt");
        let signer = Signer {
            key: &key,
            cert: &cert,
            chain: &[],
        };
        let anchors = test_anchors();
        let known = [fixture_cert("intermediate-ca.crt")];
        let der = sign(b"x", &signer, false, AT).unwrap();
        let verified = verify(&der, None, &ctx(&anchors, &known)).unwrap();
        let info = &verified.signers[0].info;
        assert_eq!(info.state, SignatureState::KeyExpired);
        assert_eq!(info.chain.as_ref().unwrap().state, ChainState::Expired);
        // Signed while the certificate was valid (2020): valid then.
        let der = sign(b"x", &signer, false, 1_600_000_000).unwrap();
        let verified = verify(&der, None, &ctx(&anchors, &known)).unwrap();
        assert_eq!(verified.signers[0].info.state, SignatureState::Valid);
        // A signing time in the future is not believed: judged now.
        let der = sign(b"x", &signer, false, AT + 10 * FUTURE_SKEW).unwrap();
        let verified = verify(&der, None, &ctx(&anchors, &known)).unwrap();
        assert_eq!(verified.signers[0].info.state, SignatureState::KeyExpired);
    }

    #[test]
    fn signing_time_uses_utc_time_until_2049() {
        assert!(matches!(signing_time_value(AT).unwrap(), Time::UtcTime(_)));
        // 2050-01-01
        assert!(matches!(
            signing_time_value(2_524_608_000).unwrap(),
            Time::GeneralTime(_)
        ));
    }

    #[test]
    fn garbage_is_an_error() {
        let anchors = test_anchors();
        assert!(verify(b"nope", None, &ctx(&anchors, &[])).is_err());
        // An EnvelopedData is not a SignedData.
        let enveloped = ContentInfo {
            content_type: oids::ID_ENVELOPED_DATA,
            content: Any::encode_from(&OctetString::new(vec![1]).unwrap()).unwrap(),
        }
        .to_der()
        .unwrap();
        assert!(verify(&enveloped, None, &ctx(&anchors, &[])).is_err());
    }
}
