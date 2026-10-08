//! TCP / TLS stream and line I/O.

use std::time::Duration;

// The longest response line accepted is `nail_net::io::MAX_LINE` (RFC 2449
// allows 512 octets; be lenient).
use nail_net::io::read_response_line;
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;

use crate::config::Pop3Config;
use crate::error::{Pop3Error, Result};

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

nail_net::delegate_stream_enum!(Pop3Stream { Plain, Tls }; Closed => closed(), Ok(()));

/// TLS handshake over `tcp`.
pub(crate) async fn handshake(config: &Pop3Config, tcp: TcpStream) -> Result<Pop3Stream> {
    let tls = with_timeout(config.timeout, async {
        nail_net::tls::connect(&config.trust, &config.host, tcp)
            .await
            .map_err(Pop3Error::from)
    })
    .await?;
    Ok(Pop3Stream::Tls(Box::new(tls)))
}

/// Runs `fut` with a timeout.
pub(crate) async fn with_timeout<T>(
    timeout: Duration,
    fut: impl Future<Output = Result<T>>,
) -> Result<T> {
    nail_net::io::with_timeout(timeout, fut, Pop3Error::Timeout).await
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
            read_response_line(reader, || Pop3Error::ConnectionLost, Pop3Error::Protocol).await
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
        drop(self.reader.get_mut().shutdown().await);
    }
}
