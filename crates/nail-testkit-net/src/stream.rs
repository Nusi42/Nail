//! The byte stream of a line-based mock server (SMTP, POP3): plain TCP that
//! can be upgraded to TLS in the middle of a session (STARTTLS / STLS).

use std::io;

use nail_net::io::{LineRead, MAX_LINE, read_line_limited};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::server::TlsStream;

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

nail_net::delegate_stream_enum!(MockStream { Plain, Tls });

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
        match read_line_limited(&mut self.reader, MAX_LINE).await? {
            LineRead::Eof => Ok(None),
            LineRead::TooLong => Err(io::Error::new(io::ErrorKind::InvalidData, "line too long")),
            LineRead::Line(mut line) => {
                if line.ends_with(b"\r") {
                    line.pop();
                }
                Ok(Some(line))
            }
        }
    }

    /// Reads the next command line as text with its upper-case verb;
    /// `None` at EOF.
    pub(crate) async fn next_command(&mut self) -> io::Result<Option<(String, String)>> {
        let Some(raw) = self.read_line().await? else {
            return Ok(None);
        };
        let line = String::from_utf8_lossy(&raw).into_owned();
        let verb = verb(&line);
        Ok(Some((line, verb)))
    }

    /// Reads exactly `n` bytes (a ManageSieve `{n+}` literal).
    pub(crate) async fn read_exact_bytes(&mut self, n: usize) -> io::Result<Vec<u8>> {
        let mut data = vec![0; n];
        self.reader.read_exact(&mut data).await?;
        Ok(data)
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

    /// Closes the connection (TLS `close_notify` when encrypted).
    pub(crate) async fn shutdown(mut self) {
        drop(self.reader.get_mut().shutdown().await);
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
        .map_or("", |(_, rest)| rest.trim())
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
