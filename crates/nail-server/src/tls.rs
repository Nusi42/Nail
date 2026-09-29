//! TLS for the remote UI and the listener that accepts connections.
//!
//! * [`server_config`]: rustls with the *ring* provider, TLS 1.3, and TLS
//!   1.2 only with forward-secret AEAD suites (ECDHE + AES-GCM /
//!   ChaCha20-Poly1305; rustls implements no RSA key exchange and no CBC).
//!   The server's suite order wins. ALPN offers `http/1.1` only (the
//!   WebSocket upgrade needs HTTP/1.1).
//! * [`Incoming`]: an [`axum::serve::Listener`] that drops connections from
//!   outside the allowed networks before any byte is read, and completes
//!   TLS handshakes in their own tasks (a slow client cannot stall
//!   `accept`), with a deadline.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::connect_info::Connected;
use axum::serve::IncomingStream;
use ipnet::IpNet;
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::{ServerConfig, SupportedCipherSuite};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpListener;
use tokio::sync::{Semaphore, mpsc};
use tokio::task::JoinHandle;

/// Deadline for a TLS handshake.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// Handshakes in progress at most (the rest wait in the kernel backlog).
const MAX_HANDSHAKES: usize = 64;

/// The cipher suites offered: TLS 1.3, then TLS 1.2 with ECDHE (forward
/// secrecy) and AEAD only.
pub fn cipher_suites() -> Vec<SupportedCipherSuite> {
    use rustls::crypto::ring::cipher_suite as cs;
    vec![
        cs::TLS13_AES_256_GCM_SHA384,
        cs::TLS13_CHACHA20_POLY1305_SHA256,
        cs::TLS13_AES_128_GCM_SHA256,
        cs::TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384,
        cs::TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256,
        cs::TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256,
        cs::TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384,
        cs::TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256,
        cs::TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256,
    ]
}

/// The *ring* provider restricted to [`cipher_suites`].
pub fn provider() -> Arc<CryptoProvider> {
    Arc::new(CryptoProvider {
        cipher_suites: cipher_suites(),
        ..rustls::crypto::ring::default_provider()
    })
}

/// A TLS configuration that failed to build.
#[derive(Debug, thiserror::Error)]
#[error("TLS configuration: {0}")]
pub struct TlsConfigError(#[from] rustls::Error);

/// The server configuration for a certificate chain and its key.
pub fn server_config(
    chain: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
) -> Result<Arc<ServerConfig>, TlsConfigError> {
    let mut config = ServerConfig::builder_with_provider(provider())
        .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])?
        .with_no_client_auth()
        .with_single_cert(chain, key)?;
    config.ignore_client_order = true;
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(Arc::new(config))
}

/// A connection (plain or TLS).
pub trait Conn: AsyncRead + AsyncWrite + Unpin + Send + 'static {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send + 'static> Conn for T {}

/// The peer address of a connection, for handlers that need it
/// (`ConnectInfo<ClientAddr>`; rate limits, audit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientAddr(pub SocketAddr);

impl Connected<IncomingStream<'_, Incoming>> for ClientAddr {
    fn connect_info(stream: IncomingStream<'_, Incoming>) -> Self {
        Self(*stream.remote_addr())
    }
}

/// Accepted connections, filtered and (optionally) TLS-wrapped by a
/// background task.
pub struct Incoming {
    rx: mpsc::Receiver<(Box<dyn Conn>, SocketAddr)>,
    local: SocketAddr,
    task: JoinHandle<()>,
}

impl std::fmt::Debug for Incoming {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Incoming")
            .field("local", &self.local)
            .finish_non_exhaustive()
    }
}

/// Whether `peer` may connect: loopback always, else any of `networks`
/// (empty = everyone who reaches the port).
pub(crate) fn peer_allowed(networks: &[IpNet], peer: std::net::IpAddr) -> bool {
    let peer = match peer {
        std::net::IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(peer, std::net::IpAddr::V4),
        v4 => v4,
    };
    peer.is_loopback() || networks.is_empty() || networks.iter().any(|n| n.contains(&peer))
}

impl Incoming {
    /// Starts accepting on `listener`.
    pub(crate) fn start(
        listener: TcpListener,
        tls: Option<Arc<ServerConfig>>,
        networks: Vec<IpNet>,
    ) -> std::io::Result<Self> {
        let local = listener.local_addr()?;
        let (tx, rx) = mpsc::channel::<(Box<dyn Conn>, SocketAddr)>(MAX_HANDSHAKES);
        let acceptor = tls.map(tokio_rustls::TlsAcceptor::from);
        let permits = Arc::new(Semaphore::new(MAX_HANDSHAKES));
        let task = tokio::spawn(async move {
            loop {
                let (stream, peer) = match listener.accept().await {
                    Ok(pair) => pair,
                    Err(err) => {
                        tracing::debug!(%err, "accept failed");
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        continue;
                    }
                };
                if !peer_allowed(&networks, peer.ip()) {
                    tracing::info!(%peer, "connection from outside the allowed networks dropped");
                    drop(stream);
                    continue;
                }
                let _ = stream.set_nodelay(true);
                match &acceptor {
                    None => {
                        if tx.send((Box::new(stream), peer)).await.is_err() {
                            return;
                        }
                    }
                    Some(acceptor) => {
                        let Ok(permit) = Arc::clone(&permits).acquire_owned().await else {
                            return;
                        };
                        let acceptor = acceptor.clone();
                        let tx = tx.clone();
                        tokio::spawn(async move {
                            let _permit = permit;
                            match tokio::time::timeout(HANDSHAKE_TIMEOUT, acceptor.accept(stream))
                                .await
                            {
                                Ok(Ok(tls)) => {
                                    let _ = tx.send((Box::new(tls), peer)).await;
                                }
                                Ok(Err(err)) => {
                                    tracing::debug!(%peer, %err, "TLS handshake failed");
                                }
                                Err(_) => tracing::debug!(%peer, "TLS handshake timed out"),
                            }
                        });
                    }
                }
            }
        });
        Ok(Self { rx, local, task })
    }
}

impl Drop for Incoming {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl axum::serve::Listener for Incoming {
    type Io = Box<dyn Conn>;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        match self.rx.recv().await {
            Some(pair) => pair,
            // The accept task ended (it only does when aborted): park.
            None => std::future::pending().await,
        }
    }

    fn local_addr(&self) -> std::io::Result<Self::Addr> {
        Ok(self.local)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_forward_secret_aead_suites() {
        for suite in cipher_suites() {
            let name = format!("{:?}", suite.suite());
            assert!(
                name.starts_with("TLS13_") || name.starts_with("TLS_ECDHE_"),
                "{name}"
            );
            assert!(
                name.contains("_GCM_") || name.contains("CHACHA20_POLY1305"),
                "{name}"
            );
        }
        assert_eq!(provider().cipher_suites.len(), 9);
    }

    #[test]
    fn peers_outside_the_allowed_networks_are_refused() {
        let nets: Vec<IpNet> = vec!["192.168.1.0/24".parse().unwrap()];
        let ok = |ip: &str| peer_allowed(&nets, ip.parse().unwrap());
        assert!(ok("192.168.1.20"));
        assert!(ok("::ffff:192.168.1.20"));
        assert!(ok("127.0.0.1"));
        assert!(ok("::1"));
        assert!(!ok("192.168.2.1"));
        assert!(!ok("10.0.0.1"));
        assert!(!ok("fe80::1"));
        assert!(peer_allowed(&[], "10.0.0.1".parse().unwrap()));
    }
}
