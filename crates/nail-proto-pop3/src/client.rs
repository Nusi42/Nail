//! The POP3 client (RFC 1939 with RFC 2449 `CAPA`, RFC 2595 `STLS`,
//! RFC 5034 `AUTH`).

use std::fmt;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use md5::{Digest as _, Md5};
use nail_api::accounts::{SaslMechanism, TlsMode};
use tokio::net::TcpStream;

use crate::config::{Pop3Config, Pop3Credentials};
use crate::error::{Pop3Error, Result, login_error};
use crate::transport::{Lines, Pop3Stream, handshake, with_timeout};

/// Largest message the client downloads (a guard against runaway servers).
pub const MAX_MESSAGE_BYTES: usize = 512 * 1024 * 1024;

/// RFC 5034: an initial response longer than this is sent after a `+`
/// continuation instead of on the `AUTH` line.
const MAX_AUTH_LINE: usize = 255;

/// The server's `CAPA` list.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Capabilities {
    /// Whether the server answered `CAPA` at all. When not, every optional
    /// command is simply tried.
    pub known: bool,
    /// The capability lines, e.g. `TOP`, `UIDL`, `SASL PLAIN XOAUTH2`.
    pub lines: Vec<String>,
}

impl Capabilities {
    /// Parses `CAPA` lines.
    pub fn from_lines<'a>(lines: impl IntoIterator<Item = &'a str>) -> Self {
        Self {
            known: true,
            lines: lines
                .into_iter()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_owned)
                .collect(),
        }
    }

    /// Whether `keyword` was announced.
    pub fn has(&self, keyword: &str) -> bool {
        self.lines.iter().any(|l| {
            l.split_whitespace()
                .next()
                .is_some_and(|k| k.eq_ignore_ascii_case(keyword))
        })
    }

    /// Whether `keyword` was announced or the list is unknown.
    pub fn may_have(&self, keyword: &str) -> bool {
        !self.known || self.has(keyword)
    }

    /// The SASL mechanisms (upper case).
    pub fn sasl_mechanisms(&self) -> Vec<String> {
        self.lines
            .iter()
            .filter(|l| {
                l.split_whitespace()
                    .next()
                    .is_some_and(|k| k.eq_ignore_ascii_case("SASL"))
            })
            .flat_map(|l| l.split_whitespace().skip(1).map(str::to_ascii_uppercase))
            .collect()
    }
}

/// A message on the server: its number in this session, its `UIDL` and its
/// size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerMessage {
    /// Message number (valid for this session only).
    pub number: u32,
    /// Unique id (stable across sessions).
    pub uid: String,
    /// Size in octets as reported by `LIST`.
    pub size: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Authorization,
    Transaction,
}

enum Reply {
    Ok(String),
    Err(String),
    Continue(String),
}

/// A connected POP3 session.
pub struct Pop3Client {
    lines: Lines,
    config: Pop3Config,
    caps: Capabilities,
    timestamp: Option<String>,
    state: State,
    encrypted: bool,
}

impl fmt::Debug for Pop3Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pop3Client")
            .field("host", &self.config.host)
            .field("port", &self.config.port)
            .field("encrypted", &self.encrypted)
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

/// The APOP timestamp (`<...@...>`) of a greeting, if any.
pub(crate) fn apop_timestamp(greeting: &str) -> Option<String> {
    let start = greeting.find('<')?;
    let end = greeting[start..].find('>')? + start;
    let stamp = &greeting[start..=end];
    stamp.contains('@').then(|| stamp.to_owned())
}

/// The APOP digest: lower-case hex MD5 of timestamp and password.
pub(crate) fn apop_digest(timestamp: &str, password: &str) -> String {
    Md5::digest(format!("{timestamp}{password}").as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn parse_reply(line: &[u8]) -> Result<Reply> {
    let text = String::from_utf8_lossy(line);
    if let Some(rest) = text.strip_prefix("+OK") {
        Ok(Reply::Ok(rest.trim().to_owned()))
    } else if let Some(rest) = text.strip_prefix("-ERR") {
        Ok(Reply::Err(rest.trim().to_owned()))
    } else if let Some(rest) = text.strip_prefix('+') {
        Ok(Reply::Continue(rest.trim().to_owned()))
    } else {
        Err(Pop3Error::Protocol(format!(
            "unexpected response: {}",
            text.chars().take(80).collect::<String>()
        )))
    }
}

fn b64(bytes: &[u8]) -> String {
    STANDARD.encode(bytes)
}

fn number(text: &str) -> Result<u32> {
    text.parse()
        .map_err(|_| Pop3Error::Protocol(format!("expected a message number, got {text:?}")))
}

impl Pop3Client {
    /// Connects, negotiates TLS as configured (never falling back from STLS
    /// to plaintext) and reads the greeting and capabilities.
    pub async fn connect(config: &Pop3Config) -> Result<Self> {
        let tcp = with_timeout(config.timeout, async {
            TcpStream::connect((config.host.as_str(), config.port))
                .await
                .map_err(|e| Pop3Error::Connect {
                    host: config.host.clone(),
                    port: config.port,
                    message: e.to_string(),
                })
        })
        .await?;
        let _ = tcp.set_nodelay(true);
        let (stream, encrypted) = match config.tls {
            TlsMode::Implicit => (handshake(config, tcp).await?, true),
            TlsMode::Starttls => (Pop3Stream::Plain(tcp), false),
            TlsMode::Plaintext => {
                tracing::warn!(host = %config.host, "POP3 connection without TLS");
                (Pop3Stream::Plain(tcp), false)
            }
        };
        let mut client = Self {
            lines: Lines::new(stream, config.timeout),
            config: config.clone(),
            caps: Capabilities::default(),
            timestamp: None,
            state: State::Authorization,
            encrypted,
        };
        let greeting = client.lines.read_line().await?;
        match parse_reply(&greeting)? {
            Reply::Ok(text) => client.timestamp = apop_timestamp(&text),
            Reply::Err(text) | Reply::Continue(text) => return Err(Pop3Error::Temporary(text)),
        }
        client.caps = client.capa().await?;
        if config.tls == TlsMode::Starttls {
            client.starttls().await?;
        }
        Ok(client)
    }

    async fn starttls(&mut self) -> Result<()> {
        if self.caps.known && !self.caps.has("STLS") {
            return Err(Pop3Error::StartTlsUnavailable);
        }
        match self.command("STLS").await? {
            Reply::Ok(_) => {}
            _ => return Err(Pop3Error::StartTlsUnavailable),
        }
        // Anything the server sent after +OK was not protected by TLS.
        if self.lines.has_buffered_data() {
            return Err(Pop3Error::Protocol("unexpected data after STLS".to_owned()));
        }
        let placeholder = Lines::new(Pop3Stream::Closed, self.config.timeout);
        let lines = std::mem::replace(&mut self.lines, placeholder);
        let Pop3Stream::Plain(tcp) = lines.into_stream() else {
            return Err(Pop3Error::Protocol(
                "STLS on an encrypted connection".to_owned(),
            ));
        };
        self.lines = Lines::new(handshake(&self.config, tcp).await?, self.config.timeout);
        self.encrypted = true;
        // RFC 2595: discard earlier capabilities and ask again.
        self.caps = self.capa().await?;
        Ok(())
    }

    /// The capabilities (after STLS: the ones announced over TLS).
    pub fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    /// Whether the session is encrypted.
    pub fn is_encrypted(&self) -> bool {
        self.encrypted
    }

    /// Whether the greeting offered APOP.
    pub fn supports_apop(&self) -> bool {
        self.timestamp.is_some()
    }

    async fn command(&mut self, line: &str) -> Result<Reply> {
        self.lines.send(line).await?;
        parse_reply(&self.lines.read_line().await?)
    }

    /// Sends a command expecting `+OK`; `-ERR` becomes [`Pop3Error::Refused`].
    async fn simple(&mut self, line: &str) -> Result<String> {
        let name = line
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_owned();
        match self.command(line).await? {
            Reply::Ok(text) => Ok(text),
            Reply::Err(message) => Err(Pop3Error::Refused {
                command: name,
                message,
            }),
            Reply::Continue(_) => Err(Pop3Error::Protocol(format!(
                "unexpected continuation to {name}"
            ))),
        }
    }

    /// Asks for the capabilities. A server without `CAPA` yields an
    /// "unknown" list.
    pub async fn capa(&mut self) -> Result<Capabilities> {
        match self.command("CAPA").await? {
            Reply::Ok(_) => {
                let body = self.lines.read_multiline(64 * 1024).await?;
                let text = String::from_utf8_lossy(&body);
                Ok(Capabilities::from_lines(text.lines()))
            }
            _ => Ok(Capabilities::default()),
        }
    }

    fn require_transaction(&self) -> Result<()> {
        if self.state == State::Transaction {
            Ok(())
        } else {
            Err(Pop3Error::Protocol("not logged in".to_owned()))
        }
    }

    /// Logs in.
    pub async fn login(&mut self, credentials: &Pop3Credentials) -> Result<()> {
        if self.state == State::Transaction {
            return Ok(());
        }
        match credentials {
            Pop3Credentials::Password {
                username,
                password,
                mechanism,
            } => match self.password_method(*mechanism)? {
                SaslMechanism::Plain => {
                    let payload = format!("\0{username}\0{password}");
                    self.sasl("PLAIN", payload.as_bytes(), "*").await?;
                }
                SaslMechanism::Apop => {
                    let timestamp = self.timestamp.clone().unwrap_or_default();
                    let digest = apop_digest(&timestamp, password);
                    match self.command(&format!("APOP {username} {digest}")).await? {
                        Reply::Ok(_) => {}
                        Reply::Err(text) | Reply::Continue(text) => return Err(login_error(&text)),
                    }
                }
                _ => {
                    match self.command(&format!("USER {username}")).await? {
                        Reply::Ok(_) => {}
                        Reply::Err(text) | Reply::Continue(text) => return Err(login_error(&text)),
                    }
                    match self.command(&format!("PASS {password}")).await? {
                        Reply::Ok(_) => {}
                        Reply::Err(text) | Reply::Continue(text) => return Err(login_error(&text)),
                    }
                }
            },
            Pop3Credentials::OAuth2 {
                username,
                access_token,
                mechanism,
            } => {
                let offered = self.caps.sasl_mechanisms();
                let pick = |m: SaslMechanism, name: &str| {
                    (mechanism.is_none_or(|forced| forced == m)
                        && (!self.caps.known || offered.iter().any(|o| o == name)))
                    .then_some(m)
                };
                let chosen = pick(SaslMechanism::Xoauth2, "XOAUTH2")
                    .or_else(|| pick(SaslMechanism::Oauthbearer, "OAUTHBEARER"))
                    .ok_or_else(|| {
                        Pop3Error::AuthUnsupported(format!(
                            "no OAuth2 SASL mechanism offered ({})",
                            offered.join(" ")
                        ))
                    })?;
                if chosen == SaslMechanism::Oauthbearer {
                    let user = username.replace('=', "=3D").replace(',', "=2C");
                    let payload = format!(
                        "n,a={user},\x01host={}\x01port={}\x01auth=Bearer {access_token}\x01\x01",
                        self.config.host, self.config.port
                    );
                    self.sasl("OAUTHBEARER", payload.as_bytes(), "AQ==").await?;
                } else {
                    let payload = format!("user={username}\x01auth=Bearer {access_token}\x01\x01");
                    self.sasl("XOAUTH2", payload.as_bytes(), "").await?;
                }
            }
        }
        self.state = State::Transaction;
        Ok(())
    }

    fn password_method(&self, forced: Option<SaslMechanism>) -> Result<SaslMechanism> {
        let sasl = self.caps.sasl_mechanisms();
        let offers_plain = sasl.iter().any(|m| m == "PLAIN");
        match forced {
            Some(SaslMechanism::Plain) => Ok(SaslMechanism::Plain),
            Some(SaslMechanism::Login) => Ok(SaslMechanism::Login),
            Some(SaslMechanism::Apop) if self.timestamp.is_some() => Ok(SaslMechanism::Apop),
            Some(SaslMechanism::Apop) => Err(Pop3Error::AuthUnsupported(
                "the server does not offer APOP".to_owned(),
            )),
            Some(other) => Err(Pop3Error::AuthUnsupported(format!(
                "{other:?} is not available for POP3 passwords"
            ))),
            None if offers_plain => Ok(SaslMechanism::Plain),
            None if self.caps.may_have("USER") => Ok(SaslMechanism::Login),
            None if self.timestamp.is_some() => Ok(SaslMechanism::Apop),
            None => Err(Pop3Error::AuthUnsupported(
                "the server offers neither SASL PLAIN, USER nor APOP".to_owned(),
            )),
        }
    }

    /// An RFC 5034 `AUTH` exchange with one client message. `abort` answers
    /// an error challenge (`*` cancels; the OAuth mechanisms send a dummy).
    async fn sasl(&mut self, mechanism: &str, payload: &[u8], abort: &str) -> Result<()> {
        let encoded = b64(payload);
        let initial = format!("AUTH {mechanism} {encoded}");
        let mut reply = if initial.len() <= MAX_AUTH_LINE {
            self.command(&initial).await?
        } else {
            match self.command(&format!("AUTH {mechanism}")).await? {
                Reply::Continue(_) => self.command(&encoded).await?,
                other => other,
            }
        };
        let mut detail = String::new();
        if let Reply::Continue(challenge) = &reply {
            detail = STANDARD
                .decode(challenge.trim())
                .map(|json| String::from_utf8_lossy(&json).into_owned())
                .unwrap_or_default();
            reply = self.command(abort).await?;
        }
        match reply {
            Reply::Ok(_) => Ok(()),
            Reply::Err(text) => Err(match login_error(&text) {
                Pop3Error::AuthFailed(text) if !detail.is_empty() => {
                    Pop3Error::AuthFailed(format!("{text} {detail}"))
                }
                other => other,
            }),
            Reply::Continue(_) => Err(Pop3Error::Protocol("endless SASL exchange".to_owned())),
        }
    }

    /// `STAT`: (message count, total size).
    pub async fn stat(&mut self) -> Result<(u32, u64)> {
        self.require_transaction()?;
        let text = self.simple("STAT").await?;
        let mut parts = text.split_whitespace();
        let count = parts.next().and_then(|c| c.parse().ok());
        let size = parts.next().and_then(|s| s.parse().ok());
        count
            .zip(size)
            .ok_or_else(|| Pop3Error::Protocol(format!("malformed STAT reply {text:?}")))
    }

    async fn listing(&mut self, command: &str) -> Result<Vec<(u32, String)>> {
        self.require_transaction()?;
        self.simple(command).await?;
        let body = self.lines.read_multiline(64 * 1024 * 1024).await?;
        let text = String::from_utf8_lossy(&body);
        text.lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| {
                let (n, value) = l.trim().split_once(' ').ok_or_else(|| {
                    Pop3Error::Protocol(format!("malformed {command} line {l:?}"))
                })?;
                Ok((number(n)?, value.trim().to_owned()))
            })
            .collect()
    }

    /// `LIST`: (number, size) of every message.
    pub async fn list(&mut self) -> Result<Vec<(u32, u64)>> {
        self.listing("LIST")
            .await?
            .into_iter()
            .map(|(n, size)| {
                let size = size
                    .split_whitespace()
                    .next()
                    .and_then(|s| s.parse().ok())
                    .ok_or_else(|| Pop3Error::Protocol(format!("malformed LIST size {size:?}")))?;
                Ok((n, size))
            })
            .collect()
    }

    /// `UIDL`: (number, unique id) of every message.
    pub async fn uidl(&mut self) -> Result<Vec<(u32, String)>> {
        if self.caps.known && !self.caps.has("UIDL") {
            return Err(Pop3Error::Unsupported("UIDL".to_owned()));
        }
        match self.listing("UIDL").await {
            Err(Pop3Error::Refused { .. }) => Err(Pop3Error::Unsupported("UIDL".to_owned())),
            other => other,
        }
    }

    /// `LIST` + `UIDL` joined: every message with number, uid and size.
    pub async fn messages(&mut self) -> Result<Vec<ServerMessage>> {
        let sizes: std::collections::HashMap<u32, u64> = self.list().await?.into_iter().collect();
        Ok(self
            .uidl()
            .await?
            .into_iter()
            .map(|(number, uid)| ServerMessage {
                number,
                size: sizes.get(&number).copied().unwrap_or(0),
                uid,
            })
            .collect())
    }

    /// `TOP n lines`: the header and the first `lines` body lines.
    pub async fn top(&mut self, number: u32, lines: u32) -> Result<Vec<u8>> {
        self.require_transaction()?;
        if self.caps.known && !self.caps.has("TOP") {
            return Err(Pop3Error::Unsupported("TOP".to_owned()));
        }
        self.simple(&format!("TOP {number} {lines}")).await?;
        self.lines.read_multiline(MAX_MESSAGE_BYTES).await
    }

    /// `RETR n`: the whole message (CRLF line ends).
    pub async fn retr(&mut self, number: u32) -> Result<Vec<u8>> {
        self.require_transaction()?;
        self.simple(&format!("RETR {number}")).await?;
        self.lines.read_multiline(MAX_MESSAGE_BYTES).await
    }

    /// `DELE n`: marks a message for deletion at [`Self::quit`].
    pub async fn dele(&mut self, number: u32) -> Result<()> {
        self.require_transaction()?;
        self.simple(&format!("DELE {number}")).await.map(|_| ())
    }

    /// `RSET`: unmarks every deletion of this session.
    pub async fn rset(&mut self) -> Result<()> {
        self.require_transaction()?;
        self.simple("RSET").await.map(|_| ())
    }

    /// `NOOP`.
    pub async fn noop(&mut self) -> Result<()> {
        self.simple("NOOP").await.map(|_| ())
    }

    /// `QUIT`: ends the session. After a login this commits the deletions
    /// (RFC 1939 UPDATE state); dropping the client without `QUIT` discards
    /// them.
    pub async fn quit(mut self) -> Result<()> {
        let result = self.simple("QUIT").await.map(|_| ());
        self.lines.shutdown().await;
        result
    }
}

/// Connects, logs in and quits: the "test" step of account setup. Never
/// fails; problems are reported in the result together with the
/// capabilities the server announced.
pub async fn test_login(
    config: &Pop3Config,
    credentials: &Pop3Credentials,
) -> nail_api::accounts::EndpointTestResult {
    let mut capabilities = Vec::new();
    let outcome: Result<()> = async {
        let mut client = Pop3Client::connect(config).await?;
        capabilities.clone_from(&client.capabilities().lines);
        client.login(credentials).await?;
        client.quit().await
    }
    .await;
    match outcome {
        Ok(()) => nail_api::accounts::EndpointTestResult {
            error: None,
            message: None,
            capabilities,
            localized: None,
        },
        Err(err) => nail_api::accounts::EndpointTestResult {
            error: Some(err.code()),
            message: Some(err.to_string()),
            capabilities,
            localized: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apop_from_rfc_1939() {
        let stamp = apop_timestamp("POP3 server ready <1896.697170952@dbc.mtview.ca.us>").unwrap();
        assert_eq!(stamp, "<1896.697170952@dbc.mtview.ca.us>");
        assert_eq!(
            apop_digest(&stamp, "tanstaaf"),
            "c4c9334bac560ecc979e58001b3e22fb"
        );
        assert_eq!(apop_timestamp("ready <no-at-sign>"), None);
        assert_eq!(apop_timestamp("ready"), None);
    }

    #[test]
    fn capabilities() {
        let caps = Capabilities::from_lines(["TOP", "UIDL", "SASL PLAIN xoauth2", "STLS"]);
        assert!(caps.has("uidl") && caps.has("STLS"));
        assert_eq!(caps.sasl_mechanisms(), ["PLAIN", "XOAUTH2"]);
        assert!(!caps.has("USER") && !caps.may_have("USER"));
        let unknown = Capabilities::default();
        assert!(!unknown.has("USER") && unknown.may_have("USER"));
    }

    #[test]
    fn replies() {
        assert!(matches!(parse_reply(b"+OK hi").unwrap(), Reply::Ok(t) if t == "hi"));
        assert!(matches!(parse_reply(b"-ERR no").unwrap(), Reply::Err(t) if t == "no"));
        assert!(matches!(parse_reply(b"+ abc").unwrap(), Reply::Continue(t) if t == "abc"));
        assert!(matches!(parse_reply(b"hello"), Err(Pop3Error::Protocol(_))));
    }
}
