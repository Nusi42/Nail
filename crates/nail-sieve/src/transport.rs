//! TCP / TLS stream of the ManageSieve client and the response tokenizer
//! (RFC 5804 section 4: atoms, quoted strings, literals `{n}` / `{n+}`,
//! parenthesised response codes).

use nail_connlog::{ConnTap, Part, WireProtocol};
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

use crate::client::{SieveConfig, TlsTrust};
use crate::error::{Result, SieveError};

/// Longest response line accepted.
const MAX_LINE: u64 = 64 * 1024;
/// Largest literal accepted from the server (a script).
pub(crate) const MAX_LITERAL: usize = 16 * 1024 * 1024;

/// Plain TCP or TLS over TCP.
#[derive(Debug)]
pub(crate) enum SieveStream {
    Plain(TcpStream),
    Tls(Box<TlsStream<TcpStream>>),
}

impl AsyncRead for SieveStream {
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

impl AsyncWrite for SieveStream {
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

/// The rustls client configuration (ring provider, TLS 1.2+).
pub(crate) fn client_config(trust: &TlsTrust, host: &str) -> Result<ClientConfig> {
    let tls_err = |message: String| SieveError::Tls {
        host: host.to_owned(),
        message,
    };
    let provider = Arc::new(ring::default_provider());
    let builder = ClientConfig::builder_with_provider(Arc::clone(&provider))
        .with_safe_default_protocol_versions()
        .map_err(|e| tls_err(e.to_string()))?;
    Ok(match trust {
        TlsTrust::Platform => {
            let verifier = rustls_platform_verifier::Verifier::new(provider)
                .map_err(|e| tls_err(e.to_string()))?;
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
                    .map_err(|e| tls_err(e.to_string()))?;
            }
            builder.with_root_certificates(roots).with_no_client_auth()
        }
    })
}

/// Runs `fut` with a timeout.
pub(crate) async fn with_timeout<T>(
    timeout: Duration,
    fut: impl Future<Output = Result<T>>,
) -> Result<T> {
    tokio::time::timeout(timeout, fut)
        .await
        .map_err(|_| SieveError::Timeout(timeout))?
}

/// TLS handshake over `tcp`; the parameters go to the connection log.
async fn handshake(config: &SieveConfig, tcp: TcpStream, tap: &ConnTap) -> Result<SieveStream> {
    let server_name = ServerName::try_from(config.host.clone())
        .map_err(|_| SieveError::InvalidServerName(config.host.clone()))?;
    let connector = TlsConnector::from(Arc::new(client_config(&config.trust, &config.host)?));
    let tls = with_timeout(config.timeout, async {
        connector
            .connect(server_name, tcp)
            .await
            .map_err(|e| SieveError::Tls {
                host: config.host.clone(),
                message: e.to_string(),
            })
    })
    .await?;
    tap.tls_session(tls.get_ref().1);
    Ok(SieveStream::Tls(Box::new(tls)))
}

/// Splits bytes written into text and literals (`{n+}` + data) for the
/// connection log, which never sees a literal's bytes (scripts).
fn written_parts(bytes: &[u8]) -> Vec<(String, Option<usize>)> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            let digits_end = bytes[i + 1..]
                .iter()
                .position(|b| !b.is_ascii_digit())
                .map_or(bytes.len(), |p| i + 1 + p);
            let rest = &bytes[digits_end.min(bytes.len())..];
            let closing = if rest.starts_with(b"+}\r\n") {
                Some(4)
            } else if rest.starts_with(b"}\r\n") {
                Some(3)
            } else {
                None
            };
            if let (Some(skip), Ok(n)) = (
                closing.filter(|_| digits_end > i + 1),
                std::str::from_utf8(&bytes[i + 1..digits_end])
                    .unwrap_or_default()
                    .parse::<usize>(),
            ) {
                parts.push((
                    String::from_utf8_lossy(&bytes[start..i]).into_owned(),
                    Some(n),
                ));
                i = (digits_end + skip + n).min(bytes.len());
                start = i;
                continue;
            }
        }
        i += 1;
    }
    parts.push((String::from_utf8_lossy(&bytes[start..]).into_owned(), None));
    parts
}

fn log_written(tap: &ConnTap, bytes: &[u8]) {
    if !tap.is_on() {
        return;
    }
    let parts = written_parts(bytes);
    let mut logged = Vec::with_capacity(parts.len() * 2);
    for (text, literal) in &parts {
        logged.push(Part::Text(text));
        if let Some(n) = literal {
            logged.push(Part::Bytes(*n));
        }
    }
    tap.sent(&logged);
}

/// A token of a server response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Token {
    /// An atom (`OK`, `NO`, `BYE`, `ACTIVE`, response code names).
    Atom(String),
    /// A quoted string or a literal.
    Str(String),
    /// `(`.
    Open,
    /// `)`.
    Close,
}

/// A buffered ManageSieve connection.
#[derive(Debug)]
pub(crate) struct Conn {
    reader: BufReader<SieveStream>,
    timeout: Duration,
    /// The connection log (ADR 0190), kept across `STARTTLS`.
    pub(crate) tap: Arc<ConnTap>,
}

impl Conn {
    /// Connects (and for implicit TLS, handshakes).
    pub(crate) async fn open(config: &SieveConfig, implicit_tls: bool) -> Result<Self> {
        let tcp = with_timeout(config.timeout, async {
            TcpStream::connect((config.host.as_str(), config.port))
                .await
                .map_err(|e| SieveError::Connect {
                    host: config.host.clone(),
                    port: config.port,
                    message: e.to_string(),
                    refused: e.kind() == std::io::ErrorKind::ConnectionRefused,
                })
        })
        .await?;
        if let Err(err) = tcp.set_nodelay(true) {
            tracing::debug!(%err, "TCP_NODELAY not set");
        }
        let tap = Arc::new(config.tap.connect(
            WireProtocol::ManageSieve,
            &config.host,
            config.port,
        ));
        let stream = if implicit_tls {
            handshake(config, tcp, &tap).await?
        } else {
            SieveStream::Plain(tcp)
        };
        Ok(Self {
            reader: BufReader::new(stream),
            timeout: config.timeout,
            tap,
        })
    }

    pub(crate) fn is_tls(&self) -> bool {
        matches!(self.reader.get_ref(), SieveStream::Tls(_))
    }

    /// Upgrades to TLS after `STARTTLS` was accepted. Refuses when the
    /// server already sent more data (it would be unauthenticated
    /// plaintext injected into the TLS session).
    pub(crate) async fn starttls(self, config: &SieveConfig) -> Result<Self> {
        if !self.reader.buffer().is_empty() {
            return Err(SieveError::Protocol(
                "data received after the STARTTLS answer".to_owned(),
            ));
        }
        let tap = self.tap;
        match self.reader.into_inner() {
            SieveStream::Plain(tcp) => Ok(Self {
                reader: BufReader::new(handshake(config, tcp, &tap).await?),
                timeout: config.timeout,
                tap,
            }),
            SieveStream::Tls(_) => Err(SieveError::Protocol("already encrypted".to_owned())),
        }
    }

    async fn read_line(&mut self) -> Result<Vec<u8>> {
        let reader = &mut self.reader;
        with_timeout(self.timeout, async move {
            let mut line = Vec::new();
            let n = reader.take(MAX_LINE).read_until(b'\n', &mut line).await?;
            if n == 0 {
                return Err(SieveError::ConnectionLost(String::new()));
            }
            if !line.ends_with(b"\n") {
                return Err(SieveError::Protocol("response line too long".to_owned()));
            }
            line.pop();
            if line.ends_with(b"\r") {
                line.pop();
            }
            Ok(line)
        })
        .await
    }

    async fn read_exact(&mut self, n: usize) -> Result<Vec<u8>> {
        let reader = &mut self.reader;
        with_timeout(self.timeout, async move {
            let mut data = vec![0; n];
            reader.read_exact(&mut data).await?;
            Ok(data)
        })
        .await
    }

    /// Reads one response line with its literals, as tokens.
    pub(crate) async fn read_tokens(&mut self) -> Result<Vec<Token>> {
        let mut tokens = Vec::new();
        // The response for the connection log: its lines, literals as sizes.
        let mut logged: Vec<(String, Option<usize>)> = Vec::new();
        loop {
            let line = self.read_line().await?;
            let line = String::from_utf8(line)
                .map_err(|_| SieveError::Protocol("response is not UTF-8".to_owned()))?;
            let parsed = tokenize(&line)?;
            logged.push((line, parsed.1));
            match parsed {
                (mut toks, None) => {
                    tokens.append(&mut toks);
                    let mut parts = Vec::with_capacity(logged.len() * 2);
                    for (text, literal) in &logged {
                        parts.push(Part::Text(text));
                        if let Some(n) = literal {
                            parts.push(Part::Bytes(*n));
                        }
                    }
                    self.tap.received(&parts);
                    return Ok(tokens);
                }
                (mut toks, Some(n)) => {
                    if n > MAX_LITERAL {
                        return Err(SieveError::Protocol(format!("literal of {n} bytes")));
                    }
                    tokens.append(&mut toks);
                    let data = self.read_exact(n).await?;
                    tokens.push(Token::Str(String::from_utf8(data).map_err(|_| {
                        SieveError::Protocol("literal is not UTF-8".to_owned())
                    })?));
                }
            }
        }
    }

    /// Writes bytes and flushes (logged with literals as sizes).
    pub(crate) async fn write(&mut self, bytes: &[u8]) -> Result<()> {
        log_written(&self.tap, bytes);
        let stream = self.reader.get_mut();
        with_timeout(self.timeout, async move {
            stream.write_all(bytes).await?;
            stream.flush().await?;
            Ok(())
        })
        .await
    }

    /// Closes the connection (TLS `close_notify` when encrypted).
    pub(crate) async fn shutdown(mut self) {
        if let Err(err) = self.reader.get_mut().shutdown().await {
            tracing::debug!(%err, "ManageSieve connection did not shut down cleanly");
        }
    }
}

/// Tokenizes one line; a trailing literal marker `{n}` / `{n+}` is returned
/// separately (its data follows on the wire).
pub(crate) fn tokenize(line: &str) -> Result<(Vec<Token>, Option<usize>)> {
    let bytes = line.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let bad = |what: &str| SieveError::Protocol(format!("{what} in {line:?}"));
    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' => i += 1,
            b'(' => {
                out.push(Token::Open);
                i += 1;
            }
            b')' => {
                out.push(Token::Close);
                i += 1;
            }
            b'"' => {
                i += 1;
                let mut s = Vec::new();
                loop {
                    match bytes.get(i) {
                        None => return Err(bad("unterminated string")),
                        Some(b'"') => {
                            i += 1;
                            break;
                        }
                        Some(b'\\') => {
                            let c = bytes.get(i + 1).ok_or_else(|| bad("unterminated string"))?;
                            s.push(*c);
                            i += 2;
                        }
                        Some(c) => {
                            s.push(*c);
                            i += 1;
                        }
                    }
                }
                out.push(Token::Str(
                    String::from_utf8(s).map_err(|_| bad("invalid UTF-8"))?,
                ));
            }
            b'{' => {
                let end = line[i..]
                    .find('}')
                    .ok_or_else(|| bad("unterminated literal"))?
                    + i;
                if end + 1 != bytes.len() {
                    return Err(bad("literal marker not at the end"));
                }
                let n = line[i + 1..end].trim_end_matches('+');
                let n: usize = n.parse().map_err(|_| bad("invalid literal length"))?;
                return Ok((out, Some(n)));
            }
            _ => {
                let start = i;
                while i < bytes.len()
                    && !matches!(bytes[i], b' ' | b'\t' | b'(' | b')' | b'"' | b'{')
                {
                    i += 1;
                }
                out.push(Token::Atom(line[start..i].to_owned()));
            }
        }
    }
    Ok((out, None))
}

/// A client string argument: quoted when it is short and single-line,
/// else a non-synchronizing literal (`{n+}`, RFC 5804 section 4).
pub(crate) fn encode_string(text: &str, out: &mut Vec<u8>) {
    if text.contains(['\r', '\n', '\0']) || text.len() > 1024 {
        out.extend_from_slice(format!("{{{}+}}\r\n", text.len()).as_bytes());
        out.extend_from_slice(text.as_bytes());
    } else {
        out.push(b'"');
        for b in text.bytes() {
            if b == b'"' || b == b'\\' {
                out.push(b'\\');
            }
            out.push(b);
        }
        out.push(b'"');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizes_responses() {
        let (t, lit) = tokenize("NO (QUOTA/MAXSIZE) \"too \\\"big\\\"\"").unwrap();
        assert_eq!(
            t,
            [
                Token::Atom("NO".into()),
                Token::Open,
                Token::Atom("QUOTA/MAXSIZE".into()),
                Token::Close,
                Token::Str("too \"big\"".into())
            ]
        );
        assert_eq!(lit, None);
        let (t, lit) = tokenize("\"nail\" ACTIVE").unwrap();
        assert_eq!(t, [Token::Str("nail".into()), Token::Atom("ACTIVE".into())]);
        assert_eq!(lit, None);
        assert_eq!(tokenize("{12}").unwrap(), (vec![], Some(12)));
        assert_eq!(
            tokenize("OK (TAG {3+}").unwrap(),
            (
                vec![
                    Token::Atom("OK".into()),
                    Token::Open,
                    Token::Atom("TAG".into())
                ],
                Some(3)
            )
        );
        for bad in ["\"open", "{3} x", "{x}", "{3"] {
            assert!(tokenize(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn written_literals_are_split_off() {
        let parts = written_parts(b"PUTSCRIPT \"nail\" {5+}\r\nkeep;\r\n");
        assert_eq!(
            parts,
            vec![
                ("PUTSCRIPT \"nail\" ".to_owned(), Some(5)),
                ("\r\n".to_owned(), None)
            ]
        );
        assert_eq!(
            written_parts(b"LOGOUT\r\n"),
            vec![("LOGOUT\r\n".to_owned(), None)]
        );
    }

    #[test]
    fn encodes_strings() {
        let mut out = Vec::new();
        encode_string("a\"b\\", &mut out);
        assert_eq!(out, b"\"a\\\"b\\\\\"");
        let mut out = Vec::new();
        encode_string("x\r\ny", &mut out);
        assert_eq!(out, b"{4+}\r\nx\r\ny");
        let mut out = Vec::new();
        encode_string(&"a".repeat(2000), &mut out);
        assert!(out.starts_with(b"{2000+}\r\n"));
    }
}
