//! The byte stream of a line-based mock server (SMTP, POP3): plain TCP that
//! can be upgraded to TLS in the middle of a session (STARTTLS / STLS).

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{
    AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader, ReadBuf,
};
use tokio::net::TcpStream;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::server::TlsStream;

/// Longest command line a mock server accepts (SMTP allows 512 octets plus
/// extensions; SASL responses can be longer).
pub(crate) const MAX_LINE: u64 = 64 * 1024;

/// Plain TCP or server-side TLS over TCP.
#[derive(Debug)]
pub(crate) enum MockStream {
    Plain(TcpStream),
    Tls(Box<TlsStream<TcpStream>>),
}

impl MockStream {
    pub(crate) fn is_tls(&self) -> bool {
        matches!(self, Self::Tls(_))
    }
}

impl AsyncRead for MockStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Plain(s) => Pin::new(s).poll_read(cx, buf),
            Self::Tls(s) => Pin::new(s.as_mut()).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for MockStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            Self::Plain(s) => Pin::new(s).poll_write(cx, buf),
            Self::Tls(s) => Pin::new(s.as_mut()).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Plain(s) => Pin::new(s).poll_flush(cx),
            Self::Tls(s) => Pin::new(s.as_mut()).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Plain(s) => Pin::new(s).poll_shutdown(cx),
            Self::Tls(s) => Pin::new(s.as_mut()).poll_shutdown(cx),
        }
    }
}

/// A buffered connection of a line-based mock server.
#[derive(Debug)]
pub(crate) struct Connection {
    reader: BufReader<MockStream>,
}

impl Connection {
    pub(crate) fn new(stream: MockStream) -> Self {
        Self {
            reader: BufReader::new(stream),
        }
    }

    /// Accepts a TLS handshake right away (implicit TLS).
    pub(crate) async fn accept_tls(tcp: TcpStream, acceptor: &TlsAcceptor) -> io::Result<Self> {
        let tls = acceptor.accept(tcp).await?;
        Ok(Self::new(MockStream::Tls(Box::new(tls))))
    }

    pub(crate) fn is_tls(&self) -> bool {
        self.reader.get_ref().is_tls()
    }

    /// Upgrades a plaintext connection to TLS. Fails when the client already
    /// sent data after the STARTTLS command (pipelined plaintext would be a
    /// command-injection vector) or when the connection is already TLS.
    pub(crate) async fn upgrade(self, acceptor: &TlsAcceptor) -> io::Result<Self> {
        if !self.reader.buffer().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "data pipelined after STARTTLS",
            ));
        }
        match self.reader.into_inner() {
            MockStream::Plain(tcp) => Self::accept_tls(tcp, acceptor).await,
            MockStream::Tls(_) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "connection is already encrypted",
            )),
        }
    }

    /// Reads one line without the trailing CRLF / LF. `None` at EOF.
    pub(crate) async fn read_line(&mut self) -> io::Result<Option<Vec<u8>>> {
        let mut line = Vec::new();
        let n = (&mut self.reader)
            .take(MAX_LINE)
            .read_until(b'\n', &mut line)
            .await?;
        if n == 0 {
            return Ok(None);
        }
        if !line.ends_with(b"\n") {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "line too long"));
        }
        line.pop();
        if line.ends_with(b"\r") {
            line.pop();
        }
        Ok(Some(line))
    }

    /// Reads a dot-terminated block (SMTP `DATA`), undoing dot-stuffing.
    /// Lines end in CRLF in the result. Fails when the block exceeds
    /// `max_bytes`; the rest is still consumed so the session can go on.
    pub(crate) async fn read_dot_block(
        &mut self,
        max_bytes: usize,
    ) -> io::Result<Result<Vec<u8>, usize>> {
        let mut data = Vec::new();
        let mut total = 0usize;
        loop {
            let Some(line) = self.read_line().await? else {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "connection closed inside DATA",
                ));
            };
            if line == b"." {
                break;
            }
            let line = line.strip_prefix(b".").unwrap_or(&line);
            total += line.len() + 2;
            if total <= max_bytes {
                data.extend_from_slice(line);
                data.extend_from_slice(b"\r\n");
            }
        }
        if total > max_bytes {
            Ok(Err(total))
        } else {
            Ok(Ok(data))
        }
    }

    /// Writes bytes and flushes.
    pub(crate) async fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        let stream = self.reader.get_mut();
        stream.write_all(bytes).await?;
        stream.flush().await
    }

    /// Writes one line with CRLF.
    pub(crate) async fn line(&mut self, text: &str) -> io::Result<()> {
        self.write(format!("{text}\r\n").as_bytes()).await
    }

    /// Closes the connection (TLS close_notify when encrypted).
    pub(crate) async fn shutdown(mut self) {
        let _ = self.reader.get_mut().shutdown().await;
    }
}

/// Upper-cases the first word of a command line.
pub(crate) fn verb(line: &str) -> String {
    line.split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase()
}

/// The text after the first word, trimmed.
pub(crate) fn args(line: &str) -> &str {
    line.trim_start()
        .split_once(char::is_whitespace)
        .map(|(_, rest)| rest.trim())
        .unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_commands() {
        assert_eq!(verb("mail FROM:<a@b>"), "MAIL");
        assert_eq!(args("mail FROM:<a@b> SIZE=10"), "FROM:<a@b> SIZE=10");
        assert_eq!(args("QUIT"), "");
        assert_eq!(verb(""), "");
    }
}
