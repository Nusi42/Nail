//! TLS for the line-based protocol clients (IMAP, POP3, SMTP,
//! ManageSieve): the trust choices and the rustls client configuration
//! (ring provider, TLS 1.2+; ADR 0014), and the client handshake.

use std::fmt;
use std::sync::Arc;

use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;
use tokio_rustls::rustls::crypto::ring;
use tokio_rustls::rustls::pki_types::{CertificateDer, ServerName};
use tokio_rustls::rustls::{ClientConfig, RootCertStore};

/// Which certificates are trusted (defined in [`crate::trust`], which
/// needs no TLS dependency).
pub use crate::trust::TlsTrust;

/// Why a TLS connection could not be set up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TlsError {
    /// The host is not a valid TLS server name.
    InvalidServerName(String),
    /// The configuration or the handshake failed (English diagnostic).
    Tls(String),
}

/// Implements `From<TlsError>` for a client's error type with the
/// variants `InvalidServerName(String)` and `Tls(String)`.
#[macro_export]
macro_rules! tls_errors {
    ($ty:ident) => {
        impl From<$crate::tls::TlsError> for $ty {
            fn from(err: $crate::tls::TlsError) -> Self {
                match err {
                    $crate::tls::TlsError::InvalidServerName(host) => Self::InvalidServerName(host),
                    $crate::tls::TlsError::Tls(text) => Self::Tls(text),
                }
            }
        }
    };
}

/// The rustls client configuration for `trust` (ring provider, TLS 1.2+).
pub fn client_config(trust: &TlsTrust) -> Result<ClientConfig, TlsError> {
    let tls = |err: &dyn fmt::Display| TlsError::Tls(err.to_string());
    let provider = Arc::new(ring::default_provider());
    let builder = ClientConfig::builder_with_provider(Arc::clone(&provider))
        .with_safe_default_protocol_versions()
        .map_err(|err| tls(&err))?;
    Ok(match trust {
        TlsTrust::Platform => {
            let verifier =
                rustls_platform_verifier::Verifier::new(provider).map_err(|err| tls(&err))?;
            builder
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(verifier))
                .with_no_client_auth()
        }
        TlsTrust::WebPkiRoots => {
            let roots = RootCertStore {
                roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
            };
            builder.with_root_certificates(roots).with_no_client_auth()
        }
        TlsTrust::CustomRoots(ders) => {
            let mut roots = RootCertStore::empty();
            for der in ders {
                roots
                    .add(CertificateDer::from(der.clone()))
                    .map_err(|err| tls(&err))?;
            }
            builder.with_root_certificates(roots).with_no_client_auth()
        }
    })
}

/// The TLS client handshake with `host` over `tcp`.
pub async fn connect(
    trust: &TlsTrust,
    host: &str,
    tcp: TcpStream,
) -> Result<TlsStream<TcpStream>, TlsError> {
    let server_name = ServerName::try_from(host.to_owned())
        .map_err(|_| TlsError::InvalidServerName(host.to_owned()))?;
    let connector = TlsConnector::from(Arc::new(client_config(trust)?));
    connector
        .connect(server_name, tcp)
        .await
        .map_err(|err| TlsError::Tls(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_all_trust_configs() {
        client_config(&TlsTrust::Platform).unwrap();
        client_config(&TlsTrust::WebPkiRoots).unwrap();
        client_config(&TlsTrust::CustomRoots(Vec::new())).unwrap();
        assert!(matches!(
            client_config(&TlsTrust::CustomRoots(vec![vec![1, 2, 3]])),
            Err(TlsError::Tls(_))
        ));
    }

    #[tokio::test]
    async fn invalid_names_are_refused_before_the_handshake() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let tcp = TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let err = connect(&TlsTrust::Platform, "not a host", tcp)
            .await
            .unwrap_err();
        assert_eq!(err, TlsError::InvalidServerName("not a host".to_owned()));
    }
}
