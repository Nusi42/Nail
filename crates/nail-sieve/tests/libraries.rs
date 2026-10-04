//! Smoke tests of the libraries WP51 builds on (WP50, ADR 0043): a rustls
//! client config with the ring provider (the ManageSieve client's TLS),
//! SASL PLAIN / XOAUTH2 encodings, script hashes and the regex dialect.
#![cfg(test)]

use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use sha2::{Digest, Sha256};

#[test]
fn rustls_client_config_uses_ring() {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(rustls::RootCertStore::empty())
        .with_no_client_auth();
    let _connector = tokio_rustls::TlsConnector::from(Arc::new(config));
}

#[test]
fn sasl_encodings() {
    // AUTHENTICATE "PLAIN": authzid NUL authcid NUL password.
    assert_eq!(
        STANDARD.encode(b"\0ada@example.org\0secret"),
        "AGFkYUBleGFtcGxlLm9yZwBzZWNyZXQ="
    );
    // XOAUTH2: user=<name> ^A auth=Bearer <token> ^A ^A.
    let xoauth = format!(
        "user={}\x01auth=Bearer {}\x01\x01",
        "ada@example.org", "tok"
    );
    assert!(
        STANDARD
            .encode(xoauth)
            .starts_with("dXNlcj1hZGFAZXhhbXBsZS5vcmc")
    );
}

#[test]
fn script_hash_and_regex() {
    let hash = hex::encode(Sha256::digest(b"keep;\n"));
    assert_eq!(hash.len(), 64);
    let re = regex::Regex::new(r"^\[list\] .*$").unwrap();
    assert!(re.is_match("[list] weekly"));
}
