//! Throw-away TLS material for the mock servers: a test CA and a server
//! certificate for `localhost` / `127.0.0.1` signed by it.

use std::sync::Arc;

use rcgen::{BasicConstraints, CertificateParams, DnType, IsCa, Issuer, KeyPair, KeyUsagePurpose};
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::ServerConfig;
use tokio_rustls::rustls::crypto::ring;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

/// TLS material of a mock server.
#[derive(Clone)]
pub struct TestTls {
    /// DER of the test CA certificate; clients trust this as a root.
    pub ca_der: Vec<u8>,
    /// Acceptor configured with the server certificate.
    pub acceptor: TlsAcceptor,
}

impl std::fmt::Debug for TestTls {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TestTls")
            .field("ca_der_len", &self.ca_der.len())
            .finish_non_exhaustive()
    }
}

/// Errors while generating the test certificates.
#[derive(Debug, thiserror::Error)]
pub enum TestTlsError {
    /// Certificate generation failed.
    #[error("generating test certificate: {0}")]
    Rcgen(#[from] rcgen::Error),
    /// The TLS server configuration was rejected.
    #[error("configuring TLS: {0}")]
    Rustls(#[from] tokio_rustls::rustls::Error),
}

impl TestTls {
    /// Generates a fresh CA and a server certificate valid for `localhost`
    /// and `127.0.0.1`.
    pub fn generate() -> Result<Self, TestTlsError> {
        let mut ca_params = CertificateParams::new(Vec::<String>::new())?;
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca_params
            .distinguished_name
            .push(DnType::CommonName, "Nail Test CA");
        ca_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        let ca_key = KeyPair::generate()?;
        let ca_cert = ca_params.self_signed(&ca_key)?;
        let issuer = Issuer::new(ca_params, ca_key);

        let mut leaf_params =
            CertificateParams::new(vec!["localhost".to_owned(), "127.0.0.1".to_owned()])?;
        leaf_params
            .distinguished_name
            .push(DnType::CommonName, "localhost");
        leaf_params.use_authority_key_identifier_extension = true;
        let leaf_key = KeyPair::generate()?;
        let leaf_cert = leaf_params.signed_by(&leaf_key, &issuer)?;

        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(leaf_key.serialize_der()));
        let config = ServerConfig::builder_with_provider(Arc::new(ring::default_provider()))
            .with_safe_default_protocol_versions()?
            .with_no_client_auth()
            .with_single_cert(vec![CertificateDer::from(leaf_cert.der().to_vec())], key)?;

        Ok(Self {
            ca_der: ca_cert.der().to_vec(),
            acceptor: TlsAcceptor::from(Arc::new(config)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_distinct_material() {
        let a = TestTls::generate().unwrap();
        let b = TestTls::generate().unwrap();
        assert!(!a.ca_der.is_empty());
        assert_ne!(a.ca_der, b.ca_der);
    }
}
