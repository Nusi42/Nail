//! TLS summaries: protocol version, cipher suite and the server
//! certificate's subject, issuer, expiry and fingerprint.

use der::Decode;
use sha2::{Digest, Sha256};
use x509_cert::Certificate;

/// `TLS TLSv1_3 TLS13_AES_256_GCM_SHA384; certificate CN=imap.example.org,
/// issued by CN=R11,O=Let's Encrypt,C=US, valid until 2026-12-01, SHA-256
/// 1a2b3c4d5e6f7081`.
pub fn note(version: &str, cipher: &str, peer_certificate: Option<&[u8]>) -> String {
    let mut text = ["TLS", version, cipher]
        .iter()
        .filter(|part| !part.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(" ");
    if let Some(summary) = peer_certificate.and_then(certificate_summary) {
        text.push_str("; ");
        text.push_str(&summary);
    }
    text
}

/// The certificate's subject, issuer, expiry and the first 8 bytes of its
/// SHA-256 fingerprint; `None` when it cannot be parsed.
pub fn certificate_summary(der: &[u8]) -> Option<String> {
    let certificate = Certificate::from_der(der).ok()?;
    let tbs = &certificate.tbs_certificate;
    let until = chrono::DateTime::from_timestamp(
        i64::try_from(tbs.validity.not_after.to_unix_duration().as_secs()).ok()?,
        0,
    )?
    .format("%Y-%m-%d");
    let fingerprint = Sha256::digest(der);
    Some(format!(
        "certificate {}, issued by {}, valid until {until}, SHA-256 {}",
        tbs.subject,
        tbs.issuer,
        hex::encode(&fingerprint[..8])
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unparsable_certificates_are_left_out() {
        assert_eq!(
            note("TLSv1_3", "TLS13_AES_128_GCM_SHA256", Some(b"junk")),
            "TLS TLSv1_3 TLS13_AES_128_GCM_SHA256"
        );
        assert_eq!(certificate_summary(&[]), None);
    }
}
