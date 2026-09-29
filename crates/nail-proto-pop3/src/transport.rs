//! TCP / TLS stream and line I/O.

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

use crate::config::{Pop3Config, TlsTrust};
use crate::error::{Pop3Error, Result};

/// Longest response line accepted (RFC 2449 allows 512 octets; be lenient).
const MAX_LINE: u64 = 64 * 1024;

/// Plain TCP or TLS over TCP.
#[derive(Debug)]
pub(crate) enum Pop3Stream {
    Plain(TcpStream),
    Tls(Box<TlsStream<TcpStream>>),
    /// Placeholder while the connection is handed to the TLS handshake.
    Closed,
}

fn closed() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::NotConnected, "connection closed")
}

impl AsyncRead for Pop3Stream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Plain(s) => Pin::new(s).poll_read(cx, buf),
            Self::Tls(s) => Pin::new(s.as_mut()).poll_read(cx, buf),
            Self::Closed => Poll::Ready(Err(closed())),
        }
    }
}

impl AsyncWrite for Pop3Stream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            Self::Plain(s) => Pin::new(s).poll_write(cx, buf),
            Self::Tls(s) => Pin::new(s.as_mut()).poll_write(cx, buf),
            Self::Closed => Poll::Ready(Err(closed())),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Plain(s) => Pin::new(s).poll_flush(cx),
            Self::Tls(s) => Pin::new(s.as_mut()).poll_flush(cx),
            Self::Closed => Poll::Ready(Ok(())),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Plain(s) => Pin::new(s).poll_shutdown(cx),
            Self::Tls(s) => Pin::new(s.as_mut()).poll_shutdown(cx),
            Self::Closed => Poll::Ready(Ok(())),
        }
    }
}

/// Builds the rustls client configuration (ring provider, TLS 1.2+).
pub(crate) fn client_config(trust: &TlsTrust) -> Result<ClientConfig> {
    let provider = Arc::new(ring::default_provider());
    let builder = ClientConfig::builder_with_provider(Arc::clone(&provider))
        .with_safe_default_protocol_versions()
        .map_err(|err| Pop3Error::Tls(err.to_string()))?;
    Ok(match trust {
        TlsTrust::Platform => {
            let verifier = rustls_platform_verifier::Verifier::new(provider)
                .map_err(|err| Pop3Error::Tls(err.to_string()))?;
            builder
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(verifier))
                .with_no_client_auth()
        }
        TlsTrust::CustomRoots(ders) => {
            let mut roots = RootCertStore::empty();
            for der in ders {
                roots
                    .add(CertificateDer::from(der.clone()))
                    .map_err(|err| Pop3Error::Tls(err.to_string()))?;
            }
            builder.with_root_certificates(roots).with_no_client_auth()
        }
    })
}

/// TLS handshake over `tcp`.
pub(crate) async fn handshake(config: &Pop3Config, tcp: TcpStream) -> Result<Pop3Stream> {
    let server_name = ServerName::try_from(config.host.clone())
        .map_err(|_| Pop3Error::InvalidServerName(config.host.clone()))?;
    let connector = TlsConnector::from(Arc::new(client_config(&config.trust)?));
    let tls = with_timeout(config.timeout, async {
        connector
            .connect(server_name, tcp)
            .await
            .map_err(|err| Pop3Error::Tls(err.to_string()))
    })
    .await?;
    Ok(Pop3Stream::Tls(Box::new(tls)))
}

/// Runs `fut` with a timeout.
pub(crate) async fn with_timeout<T>(
    timeout: Duration,
    fut: impl Future<Output = Result<T>>,
) -> Result<T> {
    tokio::time::timeout(timeout, fut)
        .await
        .map_err(|_| Pop3Error::Timeout(timeout))?
}

/// Line-oriented connection.
#[derive(Debug)]
pub(crate) struct Lines {
    pub(crate) reader: BufReader<Pop3Stream>,
    timeout: Duration,
}

impl Lines {
    pub(crate) fn new(stream: Pop3Stream, timeout: Duration) -> Self {
        Self {
            reader: BufReader::new(stream),
            timeout,
        }
    }

    /// Reads one line without CRLF (bytes, not necessarily UTF-8).
    pub(crate) async fn read_line(&mut self) -> Result<Vec<u8>> {
        let timeout = self.timeout;
        let reader = &mut self.reader;
        with_timeout(timeout, async move {
            let mut line = Vec::new();
            let n = reader.take(MAX_LINE).read_until(b'\n', &mut line).await?;
            if n == 0 {
                return Err(Pop3Error::ConnectionLost);
            }
            if !line.ends_with(b"\n") {
                return Err(Pop3Error::Protocol("response line too long".to_owned()));
            }
            line.pop();
            if line.ends_with(b"\r") {
                line.pop();
            }
            Ok(line)
        })
        .await
    }

    /// Sends one command line.
    pub(crate) async fn send(&mut self, line: &str) -> Result<()> {
        let timeout = self.timeout;
        let stream = self.reader.get_mut();
        with_timeout(timeout, async move {
            stream.write_all(format!("{line}\r\n").as_bytes()).await?;
            stream.flush().await?;
            Ok(())
        })
        .await
    }

    /// Reads a multi-line body up to the terminating `.`, undoing
    /// dot-stuffing. Lines end in CRLF in the result.
    pub(crate) async fn read_multiline(&mut self, limit: usize) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        loop {
            let line = self.read_line().await?;
            if line == b"." {
                return Ok(out);
            }
            let line = line.strip_prefix(b".").unwrap_or(&line);
            if out.len() + line.len() > limit {
                return Err(Pop3Error::Protocol(format!(
                    "response larger than {limit} bytes"
                )));
            }
            out.extend_from_slice(line);
            out.extend_from_slice(b"\r\n");
        }
    }

    pub(crate) fn has_buffered_data(&self) -> bool {
        !self.reader.buffer().is_empty()
    }

    pub(crate) fn into_stream(self) -> Pop3Stream {
        self.reader.into_inner()
    }

    pub(crate) async fn shutdown(&mut self) {
        let _ = self.reader.get_mut().shutdown().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_trust_configs() {
        client_config(&TlsTrust::Platform).unwrap();
        client_config(&TlsTrust::CustomRoots(Vec::new())).unwrap();
        assert!(matches!(
            client_config(&TlsTrust::CustomRoots(vec![vec![1, 2, 3]])),
            Err(Pop3Error::Tls(_))
        ));
    }
}
