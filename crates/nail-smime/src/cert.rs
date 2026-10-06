//! X.509 certificates: parsing (DER, PEM, PKCS#7 bundles), identity
//! (fingerprints, addresses), extensions and display summaries.

use std::fmt;

use base64::Engine as _;
use cms::cert::IssuerAndSerialNumber;
use cms::content_info::ContentInfo;
use cms::signed_data::SignedData;
use const_oid::ObjectIdentifier;
use der::{Decode, Encode};
use nail_api::security::CertificateSummary;
use sha2_legacy::{Digest, Sha256};
use x509_cert::Certificate;
use x509_cert::ext::pkix::name::GeneralName;
use x509_cert::ext::pkix::{
    AuthorityKeyIdentifier, BasicConstraints, ExtendedKeyUsage, KeyUsage, SubjectAltName,
    SubjectKeyIdentifier,
};

use crate::error::{Error, Result};
use crate::keys::PublicKey;
use crate::oids;
use crate::time::rfc3339;

/// A parsed certificate with its original DER encoding.
#[derive(Clone)]
pub struct Cert {
    der: Vec<u8>,
    parsed: Certificate,
    fingerprint: String,
}

impl PartialEq for Cert {
    fn eq(&self, other: &Self) -> bool {
        self.der == other.der
    }
}

impl Eq for Cert {}

impl fmt::Debug for Cert {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Cert")
            .field("subject", &self.subject())
            .field("fingerprint", &self.fingerprint)
            .finish_non_exhaustive()
    }
}

/// What a certificate may be used for, from key usage and extended key
/// usage (RFC 8550 section 4.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// Signing mail: `digitalSignature` or `nonRepudiation`.
    Sign,
    /// Encrypting to it: `keyEncipherment` (RSA) or `keyAgreement` (EC).
    Encrypt,
    /// Either (a key-manager display of the chain).
    Any,
}

impl Cert {
    /// Parses one DER certificate.
    pub fn from_der(der: &[u8]) -> Result<Self> {
        let cert = Certificate::from_der(der)?;
        Ok(Self::from_parts(der.to_vec(), cert))
    }

    /// Wraps an already parsed certificate (it is re-encoded; DER
    /// certificates encode to the same bytes).
    pub fn from_certificate(cert: Certificate) -> Result<Self> {
        let der = cert.to_der()?;
        Ok(Self::from_parts(der, cert))
    }

    fn from_parts(der: Vec<u8>, cert: Certificate) -> Self {
        let fingerprint = hex_upper(&Sha256::digest(&der));
        Self {
            der,
            parsed: cert,
            fingerprint,
        }
    }

    /// Every certificate in `data`: PEM (any number of `CERTIFICATE` or
    /// `PKCS7` blocks), a DER certificate, a DER PKCS#7 / CMS certs-only
    /// bundle (`.p7c`, `.p7b`), or base64 of one of the DER forms.
    pub fn parse_many(data: &[u8]) -> Result<Vec<Cert>> {
        let text = std::str::from_utf8(data).ok();
        if let Some(text) = text.filter(|t| t.contains("-----BEGIN ")) {
            return parse_pem(text);
        }
        if let Ok(cert) = Self::from_der(data) {
            return Ok(vec![cert]);
        }
        if let Ok(certs) = certs_from_pkcs7(data) {
            return Ok(certs);
        }
        if let Some(text) = text {
            let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
            if let Ok(bin) = base64::engine::general_purpose::STANDARD.decode(compact) {
                if let Ok(cert) = Self::from_der(&bin) {
                    return Ok(vec![cert]);
                }
                if let Ok(certs) = certs_from_pkcs7(&bin) {
                    return Ok(certs);
                }
            }
        }
        Err(Error::malformed("no certificate found"))
    }

    /// The DER encoding.
    pub fn der(&self) -> &[u8] {
        &self.der
    }

    /// The parsed certificate.
    pub fn inner(&self) -> &Certificate {
        &self.parsed
    }

    /// SHA-256 of the DER encoding, uppercase hex (the keyring fingerprint).
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    /// SHA-1 of the DER encoding, uppercase hex (the Windows thumbprint).
    pub fn thumbprint(&self) -> String {
        hex_upper(&sha1::Sha1::digest(&self.der))
    }

    /// Subject distinguished name (RFC 4514).
    pub fn subject(&self) -> String {
        self.parsed.tbs_certificate.subject.to_string()
    }

    /// Issuer distinguished name (RFC 4514).
    pub fn issuer(&self) -> String {
        self.parsed.tbs_certificate.issuer.to_string()
    }

    /// Serial number, uppercase hex.
    pub fn serial(&self) -> String {
        hex_upper(self.parsed.tbs_certificate.serial_number.as_bytes())
    }

    /// The subject's common name, if any.
    pub fn common_name(&self) -> Option<String> {
        name_attribute(&self.parsed.tbs_certificate.subject, oids::COMMON_NAME)
    }

    /// Start of validity, Unix seconds.
    pub fn not_before(&self) -> i64 {
        unix(self.parsed.tbs_certificate.validity.not_before)
    }

    /// End of validity, Unix seconds.
    pub fn not_after(&self) -> i64 {
        unix(self.parsed.tbs_certificate.validity.not_after)
    }

    /// Whether `at` (Unix seconds) is within the validity period.
    pub fn valid_at(&self, at: i64) -> bool {
        self.not_before() <= at && at <= self.not_after()
    }

    /// Mail addresses: `rfc822Name` subject alternative names and PKCS #9
    /// `emailAddress` subject attributes; lowercase, without duplicates.
    pub fn emails(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut push = |email: &str| {
            let email = email.trim().to_lowercase();
            if email.contains('@') && !out.contains(&email) {
                out.push(email);
            }
        };
        if let Some(san) = self.extension::<SubjectAltName>(oids::CE_SUBJECT_ALT_NAME) {
            for name in &san.0 {
                if let GeneralName::Rfc822Name(email) = name {
                    push(email.as_str());
                }
            }
        }
        for rdn in &self.parsed.tbs_certificate.subject.0 {
            for atv in rdn.0.iter() {
                if atv.oid == oids::EMAIL_ADDRESS
                    && let Some(text) = any_text(&atv.value)
                {
                    push(&text);
                }
            }
        }
        out
    }

    /// Whether the certificate names `email` (case-insensitive).
    pub fn has_email(&self, email: &str) -> bool {
        let email = email.trim().to_lowercase();
        self.emails().contains(&email)
    }

    /// The public key.
    pub fn public_key(&self) -> Result<PublicKey> {
        PublicKey::from_spki(&self.parsed.tbs_certificate.subject_public_key_info)
    }

    /// Display name of the key algorithm; the OID for unsupported ones.
    pub fn algorithm_name(&self) -> String {
        match self.public_key() {
            Ok(key) => key.algorithm_name(),
            Err(_) => self
                .parsed
                .tbs_certificate
                .subject_public_key_info
                .algorithm
                .oid
                .to_string(),
        }
    }

    /// A decoded extension, `None` when absent or undecodable.
    fn extension<T: for<'a> Decode<'a>>(&self, oid: ObjectIdentifier) -> Option<T> {
        self.parsed
            .tbs_certificate
            .extensions
            .as_ref()?
            .iter()
            .find(|e| e.extn_id == oid)
            .and_then(|e| T::from_der(e.extn_value.as_bytes()).ok())
    }

    fn has_extension(&self, oid: ObjectIdentifier) -> bool {
        self.parsed
            .tbs_certificate
            .extensions
            .as_ref()
            .is_some_and(|exts| exts.iter().any(|e| e.extn_id == oid))
    }

    /// Key usage, `None` when the extension is absent (= unrestricted).
    pub fn key_usage(&self) -> Option<KeyUsage> {
        self.extension(oids::CE_KEY_USAGE)
    }

    /// Extended key usages, `None` when absent (= unrestricted).
    pub fn extended_key_usage(&self) -> Option<Vec<ObjectIdentifier>> {
        self.extension::<ExtendedKeyUsage>(oids::CE_EXT_KEY_USAGE)
            .map(|e| e.0)
    }

    /// Basic constraints: `(is_ca, path_len)`; `None` when absent.
    pub fn basic_constraints(&self) -> Option<(bool, Option<u8>)> {
        self.extension::<BasicConstraints>(oids::CE_BASIC_CONSTRAINTS)
            .map(|b| (b.ca, b.path_len_constraint))
    }

    /// Whether it is a CA certificate (basic constraints `cA`).
    pub fn is_ca(&self) -> bool {
        self.basic_constraints().is_some_and(|(ca, _)| ca)
    }

    /// Subject key identifier.
    pub fn subject_key_id(&self) -> Option<Vec<u8>> {
        self.extension::<SubjectKeyIdentifier>(oids::CE_SUBJECT_KEY_ID)
            .map(|s| s.0.as_bytes().to_vec())
    }

    /// Authority key identifier (its key identifier part).
    pub fn authority_key_id(&self) -> Option<Vec<u8>> {
        self.extension::<AuthorityKeyIdentifier>(oids::CE_AUTHORITY_KEY_ID)
            .and_then(|a| a.key_identifier)
            .map(|k| k.as_bytes().to_vec())
    }

    /// Whether subject and issuer are the same name.
    pub fn is_self_issued(&self) -> bool {
        self.parsed.tbs_certificate.subject == self.parsed.tbs_certificate.issuer
    }

    /// Critical extensions this implementation does not process (a chain
    /// with one is not accepted, RFC 5280 section 4.2). Certificate
    /// policies are accepted and ignored (no policy is required for mail).
    pub fn unhandled_critical_extensions(&self) -> Vec<ObjectIdentifier> {
        const HANDLED: [ObjectIdentifier; 7] = [
            oids::CE_KEY_USAGE,
            oids::CE_EXT_KEY_USAGE,
            oids::CE_BASIC_CONSTRAINTS,
            oids::CE_SUBJECT_ALT_NAME,
            oids::CE_ISSUER_ALT_NAME,
            oids::CE_SUBJECT_KEY_ID,
            oids::CE_CERTIFICATE_POLICIES,
        ];
        self.parsed
            .tbs_certificate
            .extensions
            .iter()
            .flatten()
            .filter(|e| e.critical && !HANDLED.contains(&e.extn_id))
            .map(|e| e.extn_id)
            .collect()
    }

    /// Whether key usage and extended key usage allow `purpose` for mail.
    pub fn usable_for(&self, purpose: Purpose) -> bool {
        let eku_ok = self.extended_key_usage().is_none_or(|ekus| {
            ekus.contains(&oids::KP_EMAIL_PROTECTION)
                || ekus.contains(&oids::ANY_EXTENDED_KEY_USAGE)
        });
        if !eku_ok {
            return false;
        }
        let Some(ku) = self.key_usage() else {
            return true;
        };
        let sign = ku.digital_signature() || ku.non_repudiation();
        let rsa = matches!(self.public_key(), Ok(PublicKey::Rsa(_)));
        let encrypt = if rsa {
            ku.key_encipherment()
        } else {
            ku.key_agreement()
        };
        match purpose {
            Purpose::Sign => sign,
            Purpose::Encrypt => encrypt,
            Purpose::Any => sign || encrypt,
        }
    }

    /// Whether it can sign mail (usage allows it and the key is supported).
    pub fn can_sign(&self) -> bool {
        !self.is_ca() && self.public_key().is_ok() && self.usable_for(Purpose::Sign)
    }

    /// Whether mail can be encrypted to it.
    pub fn can_encrypt(&self) -> bool {
        !self.is_ca() && self.public_key().is_ok() && self.usable_for(Purpose::Encrypt)
    }

    /// Whether this certificate is for mail at all (an end-entity
    /// certificate with an address that may sign or encrypt).
    pub fn is_mail_certificate(&self) -> bool {
        !self.is_ca() && !self.emails().is_empty() && self.usable_for(Purpose::Any)
    }

    /// The display summary of the protocol.
    pub fn summary(&self) -> CertificateSummary {
        CertificateSummary {
            subject: self.subject(),
            issuer: self.issuer(),
            serial: self.serial(),
            not_before: rfc3339(self.not_before()),
            not_after: rfc3339(self.not_after()),
            fingerprint: self.fingerprint.clone(),
        }
    }

    /// Issuer and serial number (a CMS recipient / signer identifier).
    pub fn issuer_and_serial(&self) -> IssuerAndSerialNumber {
        IssuerAndSerialNumber {
            issuer: self.parsed.tbs_certificate.issuer.clone(),
            serial_number: self.parsed.tbs_certificate.serial_number.clone(),
        }
    }

    /// Whether this is the certificate named by issuer and serial.
    pub fn matches_issuer_serial(&self, id: &IssuerAndSerialNumber) -> bool {
        self.parsed.tbs_certificate.issuer == id.issuer
            && self.parsed.tbs_certificate.serial_number == id.serial_number
    }

    /// Whether this is the certificate with this subject key identifier.
    pub fn matches_key_id(&self, key_id: &[u8]) -> bool {
        self.subject_key_id().is_some_and(|k| k == key_id)
    }

    /// PEM encoding.
    pub fn to_pem(&self) -> String {
        let b64 = base64::engine::general_purpose::STANDARD.encode(&self.der);
        let mut out = String::from("-----BEGIN CERTIFICATE-----\n");
        for chunk in b64.as_bytes().chunks(64) {
            out.push_str(std::str::from_utf8(chunk).unwrap_or_default());
            out.push('\n');
        }
        out.push_str("-----END CERTIFICATE-----\n");
        out
    }

    /// Checks that `issuer`'s key made this certificate's signature.
    pub fn verify_signed_by(&self, issuer: &Cert) -> Result<()> {
        let scheme = crate::keys::SignScheme::resolve(&self.parsed.signature_algorithm, None)?;
        let tbs = self.parsed.tbs_certificate.to_der()?;
        let signature = self
            .parsed
            .signature
            .as_bytes()
            .ok_or_else(|| Error::malformed("signature is not octet-aligned"))?;
        issuer.public_key()?.verify(scheme, &tbs, signature)
    }

    /// Whether this certificate has the extension (for tests of callers).
    pub fn has_key_usage_extension(&self) -> bool {
        self.has_extension(oids::CE_KEY_USAGE)
    }
}

fn parse_pem(text: &str) -> Result<Vec<Cert>> {
    let mut certs = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("-----BEGIN ") {
        let block = &rest[start..];
        let label_end = block[11..]
            .find("-----")
            .ok_or_else(|| Error::malformed("unterminated PEM header"))?;
        let label = &block[11..11 + label_end];
        let end_marker = format!("-----END {label}-----");
        let end = block
            .find(&end_marker)
            .ok_or_else(|| Error::malformed("unterminated PEM block"))?;
        let body: String = block[11 + label_end + 5..end]
            .lines()
            .filter(|l| !l.contains(':'))
            .collect::<String>()
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        let der = base64::engine::general_purpose::STANDARD
            .decode(body)
            .map_err(|_| Error::malformed("bad base64 in PEM"))?;
        match label {
            "CERTIFICATE" | "X509 CERTIFICATE" | "TRUSTED CERTIFICATE" => {
                certs.push(Cert::from_der(&der)?);
            }
            "PKCS7" | "CMS" => certs.extend(certs_from_pkcs7(&der)?),
            _ => {} // keys and other blocks are not certificates
        }
        rest = &block[end + end_marker.len()..];
    }
    if certs.is_empty() {
        return Err(Error::malformed("no certificate in PEM data"));
    }
    Ok(certs)
}

/// Certificates of a CMS `SignedData` (certs-only bundle or a signature).
pub(crate) fn certs_from_pkcs7(der: &[u8]) -> Result<Vec<Cert>> {
    let der = crate::ber::normalize(der)?;
    let info = ContentInfo::from_der(&der)?;
    if info.content_type != oids::ID_SIGNED_DATA {
        return Err(Error::malformed("not a PKCS #7 certificate bundle"));
    }
    let signed: SignedData = info.content.decode_as()?;
    let mut out = Vec::new();
    for choice in signed.certificates.iter().flat_map(|s| s.0.iter()) {
        if let cms::cert::CertificateChoices::Certificate(cert) = choice {
            out.push(Cert::from_certificate(cert.clone())?);
        }
    }
    if out.is_empty() {
        return Err(Error::malformed("the bundle contains no certificate"));
    }
    Ok(out)
}

fn name_attribute(name: &x509_cert::name::Name, oid: ObjectIdentifier) -> Option<String> {
    name.0
        .iter()
        .flat_map(|rdn| rdn.0.iter())
        .find(|atv| atv.oid == oid)
        .and_then(|atv| any_text(&atv.value))
}

fn any_text(value: &der::Any) -> Option<String> {
    use der::asn1::{Ia5StringRef, PrintableStringRef, Utf8StringRef};
    use der::{Tag, Tagged};
    match value.tag() {
        Tag::Utf8String => value
            .decode_as::<Utf8StringRef<'_>>()
            .ok()
            .map(|s| s.to_string()),
        Tag::PrintableString => value
            .decode_as::<PrintableStringRef<'_>>()
            .ok()
            .map(|s| s.to_string()),
        Tag::Ia5String => value
            .decode_as::<Ia5StringRef<'_>>()
            .ok()
            .map(|s| s.to_string()),
        _ => std::str::from_utf8(value.value()).ok().map(str::to_owned),
    }
}

fn unix(time: x509_cert::time::Time) -> i64 {
    i64::try_from(time.to_unix_duration().as_secs()).unwrap_or(i64::MAX)
}

/// Uppercase hex.
pub(crate) fn hex_upper(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02X}");
            s
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{fixture, fixture_cert};

    #[test]
    fn parses_alice_and_reads_identity() {
        let alice = fixture_cert("alice.crt");
        assert_eq!(alice.fingerprint().len(), 64);
        assert_eq!(alice.thumbprint().len(), 40);
        assert_eq!(alice.subject(), "CN=Alice Example,O=Nail Test");
        assert_eq!(alice.issuer(), "CN=Nail Test Email CA,O=Nail Test");
        assert_eq!(alice.serial(), "03E9"); // 1001
        assert_eq!(alice.common_name().as_deref(), Some("Alice Example"));
        assert_eq!(alice.emails(), vec!["alice@example.org".to_owned()]);
        assert!(alice.has_email("Alice@Example.ORG"));
        assert_eq!(alice.algorithm_name(), "RSA 2048");
        assert!(alice.can_sign());
        assert!(alice.can_encrypt());
        assert!(!alice.is_ca());
        assert!(alice.is_mail_certificate());
        assert_eq!(alice.unhandled_critical_extensions().len(), 0);
        let summary = alice.summary();
        assert_eq!(summary.not_before, "2020-01-01T00:00:00Z");
        assert_eq!(summary.not_after, "2125-01-01T00:00:00Z");
        assert_eq!(summary.fingerprint, alice.fingerprint());
        assert!(alice.valid_at(alice.not_before()));
        assert!(!alice.valid_at(alice.not_before() - 1));
    }

    #[test]
    fn key_usage_decides_purposes() {
        let bob = fixture_cert("bob.crt");
        assert_eq!(bob.algorithm_name(), "ECDSA P-256");
        assert!(bob.can_sign() && bob.can_encrypt());
        let erin = fixture_cert("erin.crt"); // keyEncipherment only
        assert!(!erin.can_sign());
        assert!(erin.can_encrypt());
        let dave = fixture_cert("dave.crt"); // serverAuth only
        assert!(!dave.usable_for(Purpose::Any));
        assert!(!dave.is_mail_certificate());
        let ca = fixture_cert("intermediate-ca.crt");
        assert!(ca.is_ca());
        assert_eq!(ca.basic_constraints(), Some((true, Some(0))));
        assert!(!ca.can_sign());
        assert_eq!(ca.algorithm_name(), "ECDSA P-384");
    }

    #[test]
    fn chain_signatures_verify() {
        let root = fixture_cert("root-ca.crt");
        let inter = fixture_cert("intermediate-ca.crt");
        let alice = fixture_cert("alice.crt");
        assert!(root.is_self_issued());
        root.verify_signed_by(&root).unwrap();
        inter.verify_signed_by(&root).unwrap(); // RSA signs P-384
        alice.verify_signed_by(&inter).unwrap(); // ECDSA P-384 signs RSA
        assert!(alice.verify_signed_by(&root).is_err());
        assert_eq!(alice.authority_key_id(), inter.subject_key_id());
        assert!(alice.subject_key_id().is_some());
    }

    #[test]
    fn parse_many_accepts_pem_der_bundles_and_base64() {
        let pem = fixture("alice.crt");
        let alice = Cert::parse_many(&pem).unwrap();
        assert_eq!(alice.len(), 1);
        let der = alice[0].der().to_vec();
        assert_eq!(Cert::parse_many(&der).unwrap(), alice);
        let b64 = base64::engine::general_purpose::STANDARD.encode(&der);
        assert_eq!(Cert::parse_many(b64.as_bytes()).unwrap(), alice);
        // PEM round trip.
        assert_eq!(
            Cert::parse_many(alice[0].to_pem().as_bytes()).unwrap(),
            alice
        );

        let mut two = fixture("intermediate-ca.crt");
        two.extend_from_slice(&fixture("alice.key")); // a key block is skipped
        two.extend_from_slice(&pem);
        assert_eq!(Cert::parse_many(&two).unwrap().len(), 2);

        let bundle = Cert::parse_many(&fixture("alice-chain-p7c.der")).unwrap();
        assert_eq!(bundle.len(), 2);
        assert!(
            bundle
                .iter()
                .any(|c| c.fingerprint() == alice[0].fingerprint())
        );

        assert!(Cert::parse_many(b"hello").is_err());
        assert!(Cert::parse_many(&fixture("alice.key")).is_err());
        assert!(Cert::parse_many(b"-----BEGIN CERTIFICATE-----\nAAAA").is_err());
    }

    #[test]
    fn a_tampered_certificate_fails_its_signature() {
        let inter = fixture_cert("intermediate-ca.crt");
        let alice = fixture_cert("alice.crt");
        let mut der = alice.der().to_vec();
        // Flip a byte inside the subject's common name.
        let pos = der
            .windows(5)
            .position(|w| w == b"Alice")
            .expect("name in DER");
        der[pos] = b'M';
        let forged = Cert::from_der(&der).unwrap();
        assert_ne!(forged.fingerprint(), alice.fingerprint());
        assert!(forged.verify_signed_by(&inter).is_err());
    }

    #[test]
    fn hex_is_uppercase() {
        assert_eq!(hex_upper(&[0x0a, 0xff]), "0AFF");
    }
}
