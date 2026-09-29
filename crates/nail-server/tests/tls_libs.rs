//! Smoke tests of the TLS libraries WP42 builds on (WP40, ADR 0036): a
//! self-signed certificate from `rcgen` served by `rustls` (ring provider,
//! TLS 1.3 / 1.2 only) completes a handshake with a client that pins it;
//! `instant-acme` builds with ring (no aws-lc) and knows Let's Encrypt.

use std::sync::Arc;

use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

#[tokio::test]
async fn self_signed_certificate_handshake_over_tls13() {
    let certified =
        rcgen::generate_simple_self_signed(vec!["nail.local".to_owned(), "127.0.0.1".to_owned()])
            .unwrap();
    let cert_der: CertificateDer<'static> = certified.cert.der().clone();
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
        certified.signing_key.serialize_der(),
    ));
    assert!(
        certified
            .cert
            .pem()
            .starts_with("-----BEGIN CERTIFICATE-----")
    );

    let server = rustls::ServerConfig::builder_with_provider(provider())
        .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der.clone()], key)
        .unwrap();
    let mut roots = rustls::RootCertStore::empty();
    roots.add(cert_der).unwrap();
    let client = rustls::ClientConfig::builder_with_provider(provider())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();

    let (client_io, server_io) = tokio::io::duplex(16 * 1024);
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(server));
    let connector = tokio_rustls::TlsConnector::from(Arc::new(client));
    let server_task = tokio::spawn(async move {
        let mut tls = acceptor.accept(server_io).await.unwrap();
        let mut buf = [0_u8; 4];
        tls.read_exact(&mut buf).await.unwrap();
        tls.write_all(b"pong").await.unwrap();
        tls.flush().await.unwrap();
        buf
    });
    let name = ServerName::try_from("nail.local").unwrap();
    let mut tls = connector.connect(name, client_io).await.unwrap();
    assert_eq!(
        tls.get_ref().1.protocol_version(),
        Some(rustls::ProtocolVersion::TLSv1_3)
    );
    tls.write_all(b"ping").await.unwrap();
    tls.flush().await.unwrap();
    let mut reply = [0_u8; 4];
    tls.read_exact(&mut reply).await.unwrap();
    assert_eq!(&reply, b"pong");
    assert_eq!(&server_task.await.unwrap(), b"ping");
}

#[test]
fn acme_client_knows_lets_encrypt() {
    assert_eq!(
        instant_acme::LetsEncrypt::Production.url(),
        nail_api::remote::LETS_ENCRYPT_DIRECTORY
    );
    let account = instant_acme::NewAccount {
        contact: &["mailto:admin@example.org"],
        terms_of_service_agreed: true,
        only_return_existing: false,
    };
    assert!(account.terms_of_service_agreed);
}
