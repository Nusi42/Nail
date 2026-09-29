//! Transport: TCP, TLS (implicit or via STARTTLS) and the pre-authentication
//! exchange (greeting, `CAPABILITY`, `STARTTLS`).
//!
//! The pre-auth exchange is done here on the raw stream, before `async-imap`
//! takes over, so that the capability list is known before choosing an
//! authentication mechanism and so that nothing buffered before `STARTTLS`
//! can leak into the TLS session.

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::io::{
    AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader, ReadBuf,
};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;
use tokio_rustls::rustls::crypto::ring;
use tokio_rustls::rustls::pki_types::{CertificateDer, ServerName};
use tokio_rustls::rustls::{ClientConfig, RootCertStore};

use crate::capabilities::Capabilities;
use crate::config::{ImapConfig, Security, TlsTrust};
use crate::error::{ImapError, Result};

/// Maximum length of a pre-auth response line.
const MAX_LINE: u64 = 64 * 1024;
/// Tag used for the pre-auth commands (async-imap uses `A0001`, ... later).
const PREAUTH_TAG: &str = "N0";

/// The byte stream under an IMAP session: plain TCP or TLS over TCP.
#[derive(Debug)]
pub enum ImapStream {
    /// Plain TCP (tests / explicit opt-in only).
    Plain(TcpStream),
    /// TLS over TCP.
    Tls(Box<TlsStream<TcpStream>>),
}

impl ImapStream {
    /// Whether the stream is encrypted.
    pub fn is_tls(&self) -> bool {
        matches!(self, Self::Tls(_))
    }
}

impl AsyncRead for ImapStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Plain(s) => Pin::new(s).poll_read(cx, buf),
            Self::Tls(s) => Pin::new(s.as_mut()).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for ImapStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            Self::Plain(s) => Pin::new(s).poll_write(cx, buf),
            Self::Tls(s) => Pin::new(s.as_mut()).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Plain(s) => Pin::new(s).poll_flush(cx),
            Self::Tls(s) => Pin::new(s.as_mut()).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Plain(s) => Pin::new(s).poll_shutdown(cx),
            Self::Tls(s) => Pin::new(s.as_mut()).poll_shutdown(cx),
        }
    }
}

/// Runs `fut` with the configured timeout.
pub(crate) async fn with_timeout<T>(
    timeout: Duration,
    fut: impl Future<Output = Result<T>>,
) -> Result<T> {
    tokio::time::timeout(timeout, fut)
        .await
        .map_err(|_| ImapError::Timeout(timeout))?
}

/// Connects, negotiates TLS as configured and reads the capabilities.
/// Returns a stream positioned right before authentication.
pub(crate) async fn establish(config: &ImapConfig) -> Result<(ImapStream, Capabilities)> {
    let tcp = with_timeout(config.timeout, async {
        TcpStream::connect((config.host.as_str(), config.port))
            .await
            .map_err(|source| ImapError::Connect {
                host: config.host.clone(),
                port: config.port,
                source,
            })
    })
    .await?;
    let _ = tcp.set_nodelay(true);

    match config.security {
        Security::ImplicitTls => {
            let tls = with_timeout(config.timeout, tls_handshake(config, tcp)).await?;
            let mut stream = BufReader::new(ImapStream::Tls(Box::new(tls)));
            let caps = with_timeout(config.timeout, preauth(&mut stream)).await?;
            Ok((unwrap_buffered(stream)?, caps))
        }
        Security::InsecurePlaintext => {
            tracing::warn!(host = %config.host, "IMAP connection without TLS");
            let mut stream = BufReader::new(ImapStream::Plain(tcp));
            let caps = with_timeout(config.timeout, preauth(&mut stream)).await?;
            Ok((unwrap_buffered(stream)?, caps))
        }
        Security::StartTls => {
            let mut stream = BufReader::new(tcp);
            with_timeout(config.timeout, async {
                read_greeting(&mut stream).await?;
                starttls(&mut stream).await
            })
            .await?;
            // Discard anything the server sent after the tagged OK: it was not
            // protected by TLS (STARTTLS response injection).
            let tcp = stream.into_inner();
            let tls = with_timeout(config.timeout, tls_handshake(config, tcp)).await?;
            let mut stream = BufReader::new(ImapStream::Tls(Box::new(tls)));
            let caps = with_timeout(config.timeout, capability(&mut stream)).await?;
            Ok((unwrap_buffered(stream)?, caps))
        }
    }
}

fn unwrap_buffered(stream: BufReader<ImapStream>) -> Result<ImapStream> {
    if !stream.buffer().is_empty() {
        return Err(ImapError::Protocol(
            "unexpected data before authentication".to_owned(),
        ));
    }
    Ok(stream.into_inner())
}

async fn tls_handshake(config: &ImapConfig, tcp: TcpStream) -> Result<TlsStream<TcpStream>> {
    let server_name = ServerName::try_from(config.host.clone())
        .map_err(|_| ImapError::InvalidServerName(config.host.clone()))?;
    let connector = TlsConnector::from(Arc::new(client_config(&config.tls_trust)?));
    connector
        .connect(server_name, tcp)
        .await
        .map_err(|err| ImapError::Tls(err.to_string()))
}

/// Builds the rustls client configuration (ring provider, TLS 1.2+).
pub(crate) fn client_config(trust: &TlsTrust) -> Result<ClientConfig> {
    let provider = Arc::new(ring::default_provider());
    let builder = ClientConfig::builder_with_provider(Arc::clone(&provider))
        .with_safe_default_protocol_versions()
        .map_err(|err| ImapError::Tls(err.to_string()))?;
    let config = match trust {
        TlsTrust::Platform => {
            let verifier = rustls_platform_verifier::Verifier::new(provider)
                .map_err(|err| ImapError::Tls(err.to_string()))?;
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
                    .map_err(|err| ImapError::Tls(err.to_string()))?;
            }
            builder.with_root_certificates(roots).with_no_client_auth()
        }
    };
    Ok(config)
}

async fn read_line<S: AsyncRead + Unpin>(stream: &mut BufReader<S>) -> Result<String> {
    let mut line = Vec::new();
    let n = (&mut *stream)
        .take(MAX_LINE)
        .read_until(b'\n', &mut line)
        .await?;
    if n == 0 {
        return Err(ImapError::ConnectionLost);
    }
    if !line.ends_with(b"\n") {
        return Err(ImapError::Protocol("response line too long".to_owned()));
    }
    let text = String::from_utf8_lossy(&line);
    Ok(text.trim_end_matches(['\r', '\n']).to_owned())
}

async fn read_greeting<S: AsyncRead + Unpin>(stream: &mut BufReader<S>) -> Result<()> {
    let greeting = read_line(stream).await?;
    let upper = greeting.to_ascii_uppercase();
    if upper.starts_with("* OK") {
        Ok(())
    } else if upper.starts_with("* PREAUTH") {
        Err(ImapError::Protocol(
            "PREAUTH greetings are not supported".to_owned(),
        ))
    } else if upper.starts_with("* BYE") {
        Err(ImapError::No(greeting))
    } else {
        Err(ImapError::Protocol(format!(
            "unexpected greeting: {greeting}"
        )))
    }
}

async fn preauth<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut BufReader<S>,
) -> Result<Capabilities> {
    read_greeting(stream).await?;
    capability(stream).await
}

/// Sends a tagged command and collects untagged lines until the tagged reply.
async fn command<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut BufReader<S>,
    command: &str,
) -> Result<(Vec<String>, String)> {
    let inner = stream.get_mut();
    inner
        .write_all(format!("{PREAUTH_TAG} {command}\r\n").as_bytes())
        .await?;
    inner.flush().await?;
    let mut untagged = Vec::new();
    loop {
        let line = read_line(stream).await?;
        if let Some(rest) = line
            .strip_prefix(PREAUTH_TAG)
            .and_then(|r| r.strip_prefix(' '))
        {
            return Ok((untagged, rest.to_owned()));
        }
        untagged.push(line);
    }
}

async fn capability<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut BufReader<S>,
) -> Result<Capabilities> {
    let (untagged, status) = command(stream, "CAPABILITY").await?;
    if !status.to_ascii_uppercase().starts_with("OK") {
        return Err(ImapError::Protocol(format!("CAPABILITY failed: {status}")));
    }
    untagged
        .iter()
        .find_map(|line| {
            let upper = line.to_ascii_uppercase();
            upper
                .starts_with("* CAPABILITY ")
                .then(|| Capabilities::from_atoms(line["* CAPABILITY ".len()..].split_whitespace()))
        })
        .ok_or_else(|| ImapError::Protocol("no CAPABILITY response".to_owned()))
}

async fn starttls<S: AsyncRead + AsyncWrite + Unpin>(stream: &mut BufReader<S>) -> Result<()> {
    let (_, status) = command(stream, "STARTTLS").await?;
    if status.to_ascii_uppercase().starts_with("OK") {
        Ok(())
    } else {
        Err(ImapError::StartTlsUnavailable)
    }
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
            Err(ImapError::Tls(_))
        ));
    }

    #[tokio::test]
    async fn preauth_parses_capabilities() {
        let (client, mut server) = tokio::io::duplex(4096);
        let server_task = tokio::spawn(async move {
            server.write_all(b"* OK hello\r\n").await.unwrap();
            let mut buf = [0u8; 64];
            let n = server.read(&mut buf).await.unwrap();
            assert_eq!(&buf[..n], b"N0 CAPABILITY\r\n");
            server
                .write_all(b"* CAPABILITY IMAP4rev1 AUTH=PLAIN SPECIAL-USE\r\nN0 OK done\r\n")
                .await
                .unwrap();
        });
        let mut stream = BufReader::new(client);
        let caps = preauth(&mut stream).await.unwrap();
        assert!(caps.has("imap4rev1"));
        assert!(caps.has_auth("PLAIN"));
        server_task.await.unwrap();
    }

    #[tokio::test]
    async fn rejects_bye_greeting() {
        let (client, mut server) = tokio::io::duplex(4096);
        server.write_all(b"* BYE go away\r\n").await.unwrap();
        let mut stream = BufReader::new(client);
        assert!(matches!(preauth(&mut stream).await, Err(ImapError::No(_))));
    }

    #[tokio::test]
    async fn eof_is_connection_lost() {
        let (client, server) = tokio::io::duplex(4096);
        drop(server);
        let mut stream = BufReader::new(client);
        assert!(matches!(
            preauth(&mut stream).await,
            Err(ImapError::ConnectionLost)
        ));
    }
}
