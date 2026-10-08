//! The authenticated IMAP connection: framing of server responses (lines
//! and literals), parsing with `imap-proto`, command tags and
//! `COMPRESS=DEFLATE` (RFC 4978).
//!
//! [`Connection::read`] is cancel-safe: all partial state lives in the
//! connection, so it can be raced against a timer or a stop signal (as
//! `IDLE` does) without losing data.

use std::borrow::Cow;
use std::pin::Pin;
use std::task::Poll;
use std::time::Duration;

use imap_proto::{RequestId, Response, Status};
use nail_net::deflate::DeflateIo;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};

use crate::error::{ImapError, Result};
use crate::transport::{IdleTimeout, ImapStream, ReadTimeout};

/// Longest accepted response line (between literals). `SEARCH` results of
/// large mailboxes are long lines.
const MAX_LINE: usize = 16 << 20;
/// Largest accepted response, literals included (one message body).
const MAX_RESPONSE: usize = nail_net::io::MAX_MESSAGE;

/// The stream under the connection.
type Raw = IdleTimeout<ImapStream>;

/// One response from the server.
#[derive(Debug)]
pub(crate) enum Incoming {
    /// A parsed response (tagged, untagged or continuation request).
    Response(Response<'static>),
    /// An untagged response `imap-proto` does not know (e.g. an extension we
    /// did not ask for); logged and ignored.
    Unparsed,
}

/// The byte stream, optionally compressed.
enum Link {
    Plain(Raw),
    Deflate(Box<DeflateStream<Raw>>),
    /// Only while switching to compression.
    Switching,
}

impl std::fmt::Debug for Link {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Plain(_) => "Plain",
            Self::Deflate(_) => "Deflate",
            Self::Switching => "Switching",
        })
    }
}

fn switching_error() -> std::io::Error {
    std::io::Error::other("connection is switching to compression")
}

nail_net::delegate_stream_enum!(Link { Plain, Deflate }; Switching => switching_error(), Err(switching_error()));

/// `T` compressed with raw DEFLATE in both directions. A flush emits a
/// sync-flush block, so the server can decode each command at once.
///
/// When the server closes the connection, the DEFLATE stream ends in the
/// middle of a block and the decoder reports an error; that is reported as
/// a plain end of stream instead, so it surfaces as
/// [`ImapError::ConnectionLost`] like on an uncompressed connection.
pub(crate) struct DeflateStream<T: AsyncRead + AsyncWrite + Unpin> {
    inner: DeflateIo<EofProbe<T>>,
}

impl<T: AsyncRead + AsyncWrite + Unpin> DeflateStream<T> {
    pub(crate) fn new(stream: T) -> Self {
        let probe = EofProbe {
            inner: stream,
            eof: false,
        };
        Self {
            inner: DeflateIo::new(probe),
        }
    }

    fn peer_closed(&self) -> bool {
        self.inner.get_ref().eof
    }
}

impl<T: AsyncRead + AsyncWrite + Unpin> AsyncRead for DeflateStream<T> {
    nail_net::poll_read_with!(|this, cx, buf| {
        match Pin::new(&mut this.inner).poll_read(cx, buf) {
            Poll::Ready(Err(err))
                if this.peer_closed() && err.kind() != std::io::ErrorKind::TimedOut =>
            {
                Poll::Ready(Err(std::io::ErrorKind::UnexpectedEof.into()))
            }
            other => other,
        }
    });
}

impl<T: AsyncRead + AsyncWrite + Unpin> AsyncWrite for DeflateStream<T> {
    nail_net::forward_async_write!(inner);
}

/// Remembers whether the peer closed the stream (a read returned no data
/// or failed).
struct EofProbe<T> {
    inner: T,
    eof: bool,
}

impl<T: AsyncRead + Unpin> AsyncRead for EofProbe<T> {
    nail_net::poll_read_with!(|this, cx, buf| {
        let before = buf.filled().len();
        let result = Pin::new(&mut this.inner).poll_read(cx, buf);
        match &result {
            Poll::Ready(Ok(())) if buf.filled().len() == before => this.eof = true,
            Poll::Ready(Err(err)) if err.kind() != std::io::ErrorKind::TimedOut => this.eof = true,
            _ => {}
        }
        result
    });
}

impl<T: AsyncWrite + Unpin> AsyncWrite for EofProbe<T> {
    nail_net::forward_async_write!(inner);
}

/// A connection after the pre-authentication exchange.
#[derive(Debug)]
pub(crate) struct Connection {
    reader: BufReader<Link>,
    /// The response being assembled (cancel-safe state).
    partial: Vec<u8>,
    /// Where the current line starts in `partial`.
    line_start: usize,
    /// Literal bytes still to read for the current response.
    literal_left: usize,
    next_tag: u32,
    read_timeout: ReadTimeout,
}

impl Connection {
    pub(crate) fn new(stream: ImapStream, timeout: Duration) -> Self {
        let read_timeout = ReadTimeout::new(timeout);
        Self {
            reader: BufReader::new(Link::Plain(IdleTimeout::new(stream, read_timeout.clone()))),
            partial: Vec::new(),
            line_start: 0,
            literal_left: 0,
            next_tag: 1,
            read_timeout,
        }
    }

    /// The idle timeout of reads (switched off during `IDLE`).
    pub(crate) fn read_timeout(&self) -> &ReadTimeout {
        &self.read_timeout
    }

    /// A fresh command tag (`A0001`, `A0002`, ...).
    pub(crate) fn next_tag(&mut self) -> String {
        let tag = format!("A{:04}", self.next_tag);
        self.next_tag = self.next_tag.wrapping_add(1);
        tag
    }

    pub(crate) fn is_compressed(&self) -> bool {
        matches!(self.reader.get_ref(), Link::Deflate(_))
    }

    /// Writes and flushes bytes.
    pub(crate) async fn write(&mut self, bytes: &[u8]) -> Result<()> {
        let stream = self.reader.get_mut();
        stream.write_all(bytes).await?;
        stream.flush().await?;
        Ok(())
    }

    /// Switches both directions to DEFLATE, right after the tagged `OK` of
    /// `COMPRESS DEFLATE`. Anything already buffered was sent uncompressed
    /// after the `OK`, which the protocol forbids.
    pub(crate) fn start_compression(&mut self) -> Result<()> {
        if !self.reader.buffer().is_empty() || !self.partial.is_empty() {
            return Err(ImapError::Protocol(
                "data after the COMPRESS response".to_owned(),
            ));
        }
        let reader = std::mem::replace(&mut self.reader, BufReader::new(Link::Switching));
        let link = match reader.into_inner() {
            Link::Plain(raw) => Link::Deflate(Box::new(DeflateStream::new(raw))),
            other => other,
        };
        self.reader = BufReader::new(link);
        Ok(())
    }

    /// Reads the next complete response. Cancel-safe.
    pub(crate) async fn read(&mut self) -> Result<Incoming> {
        loop {
            if self.literal_left > 0 {
                let buf = self.reader.fill_buf().await?;
                if buf.is_empty() {
                    return Err(ImapError::ConnectionLost);
                }
                let n = buf.len().min(self.literal_left);
                self.partial.extend_from_slice(&buf[..n]);
                self.reader.consume(n);
                self.literal_left -= n;
                if self.literal_left == 0 {
                    self.line_start = self.partial.len();
                }
                continue;
            }
            let line_len = self.partial.len() - self.line_start;
            let allowance = MAX_LINE.saturating_sub(line_len);
            let n = (&mut self.reader)
                .take(allowance as u64)
                .read_until(b'\n', &mut self.partial)
                .await?;
            if !self.partial.ends_with(b"\n") || self.partial.len() == self.line_start {
                if self.partial.len() - self.line_start >= MAX_LINE {
                    return Err(ImapError::Protocol("response line too long".to_owned()));
                }
                if n == 0 {
                    return Err(ImapError::ConnectionLost);
                }
                continue;
            }
            if let Some(len) = literal_length(&self.partial[self.line_start..]) {
                if self.partial.len() + len > MAX_RESPONSE {
                    return Err(ImapError::Protocol("response too large".to_owned()));
                }
                self.literal_left = len;
                self.line_start = self.partial.len();
            } else {
                let bytes = std::mem::take(&mut self.partial);
                self.line_start = 0;
                return Ok(parse(&bytes));
            }
        }
    }
}

/// The length of the literal a line announces (`... {123}\r\n`).
fn literal_length(line: &[u8]) -> Option<usize> {
    let line = line
        .strip_suffix(b"\r\n")
        .or_else(|| line.strip_suffix(b"\n"))?;
    let line = line.strip_suffix(b"}")?;
    let open = line.iter().rposition(|&b| b == b'{')?;
    let digits = &line[open + 1..];
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(digits).ok()?.parse().ok()
}

/// Parses one complete response; tagged and continuation lines that
/// `imap-proto` rejects are still recognized, other unknown responses are
/// returned as [`Incoming::Unparsed`].
fn parse(bytes: &[u8]) -> Incoming {
    match Response::from_bytes(bytes) {
        Ok(([], response)) => Incoming::Response(response.into_owned()),
        _ => fallback(bytes),
    }
}

fn fallback(bytes: &[u8]) -> Incoming {
    let text = String::from_utf8_lossy(bytes)
        .trim_end_matches(['\r', '\n'])
        .to_owned();
    if let Some(rest) = text.strip_prefix('+') {
        return Incoming::Response(Response::Continue {
            code: None,
            information: Some(Cow::Owned(rest.trim_start().to_owned())),
        });
    }
    if !text.starts_with('*') {
        let mut parts = text.splitn(3, ' ');
        if let (Some(tag), Some(status)) = (parts.next(), parts.next()) {
            let status = match status.to_ascii_uppercase().as_str() {
                "OK" => Some(Status::Ok),
                "NO" => Some(Status::No),
                "BAD" => Some(Status::Bad),
                _ => None,
            };
            if let Some(status) = status {
                return Incoming::Response(Response::Done {
                    tag: RequestId(tag.to_owned()),
                    status,
                    code: None,
                    information: parts.next().map(|t| Cow::Owned(t.to_owned())),
                });
            }
        }
    }
    tracing::debug!(response = %text, "unparsed IMAP response");
    Incoming::Unparsed
}

#[cfg(test)]
mod tests {
    use imap_proto::MailboxDatum;
    use tokio::io::AsyncWriteExt;
    use tokio::net::{TcpListener, TcpStream};

    use super::*;

    async fn pair() -> (Connection, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (client, server) = tokio::join!(TcpStream::connect(addr), listener.accept());
        let conn = Connection::new(ImapStream::Plain(client.unwrap()), Duration::from_secs(5));
        (conn, server.unwrap().0)
    }

    #[test]
    fn literal_lengths() {
        assert_eq!(literal_length(b"* 1 FETCH (BODY[] {12}\r\n"), Some(12));
        assert_eq!(literal_length(b"* 1 FETCH (BODY[] {0}\r\n"), Some(0));
        assert_eq!(literal_length(b"* OK {x}\r\n"), None);
        assert_eq!(literal_length(b"* OK done\r\n"), None);
        assert_eq!(literal_length(b"* OK {}\r\n"), None);
    }

    #[test]
    fn fallback_recognizes_tagged_and_continuations() {
        match parse(b"A0001 OK [WEIRD-CODE (x] done\r\n") {
            Incoming::Response(Response::Done {
                tag,
                status: Status::Ok,
                ..
            }) => assert_eq!(tag.0, "A0001"),
            other => panic!("{other:?}"),
        }
        match parse(b"+ idling\r\n") {
            Incoming::Response(Response::Continue { information, .. }) => {
                assert_eq!(information.as_deref(), Some("idling"));
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            parse(b"* ESEARCH (TAG \"A1\") UID ALL 1:3\r\n"),
            Incoming::Unparsed
        ));
    }

    #[tokio::test]
    async fn reads_literals_split_across_packets() {
        let (mut conn, mut server) = pair().await;
        let writer = tokio::spawn(async move {
            for chunk in [
                &b"* 1 FETCH (UID 7 BODY[] {1"[..],
                b"1}\r\nhello",
                b" world)\r\n* 3 EXI",
                b"STS\r\n",
            ] {
                server.write_all(chunk).await.unwrap();
                server.flush().await.unwrap();
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            server
        });
        match conn.read().await.unwrap() {
            Incoming::Response(Response::Fetch(1, attributes)) => assert_eq!(attributes.len(), 2),
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            conn.read().await.unwrap(),
            Incoming::Response(Response::MailboxData(MailboxDatum::Exists(3)))
        ));
        drop(writer.await.unwrap());
        assert!(matches!(conn.read().await, Err(ImapError::ConnectionLost)));
    }

    #[tokio::test]
    async fn read_is_cancel_safe() {
        let (mut conn, mut server) = pair().await;
        server.write_all(b"* 4 EXI").await.unwrap();
        // The read times out mid-line; nothing may be lost.
        let first = tokio::time::timeout(Duration::from_millis(50), conn.read()).await;
        assert!(first.is_err());
        server.write_all(b"STS\r\n").await.unwrap();
        assert!(matches!(
            conn.read().await.unwrap(),
            Incoming::Response(Response::MailboxData(MailboxDatum::Exists(4)))
        ));
    }

    #[tokio::test]
    async fn compression_round_trip() {
        let (mut conn, server) = pair().await;
        conn.start_compression().unwrap();
        assert!(conn.is_compressed());
        let mut server = DeflateStream::new(server);
        server.write_all(b"* 2 EXISTS\r\n").await.unwrap();
        server.flush().await.unwrap();
        assert!(matches!(
            conn.read().await.unwrap(),
            Incoming::Response(Response::MailboxData(MailboxDatum::Exists(2)))
        ));
        conn.write(b"A0001 NOOP\r\n").await.unwrap();
        let mut buf = [0u8; 12];
        server.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"A0001 NOOP\r\n");
    }

    #[tokio::test]
    async fn tags_increase() {
        let (mut conn, _server) = pair().await;
        assert_eq!(conn.next_tag(), "A0001");
        assert_eq!(conn.next_tag(), "A0002");
    }
}
