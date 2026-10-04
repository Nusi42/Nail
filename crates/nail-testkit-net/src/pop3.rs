//! An in-process mock POP3 server (RFC 1939, RFC 2449 `CAPA`, RFC 2595
//! `STLS`, RFC 5034 `AUTH`).
//!
//! Supported: `CAPA`, `STLS`, `USER` / `PASS`, `APOP`, `AUTH PLAIN` /
//! `XOAUTH2` / `OAUTHBEARER` (with or without initial response), `STAT`,
//! `LIST`, `UIDL`, `TOP`, `RETR`, `DELE`, `RSET`, `NOOP`, `QUIT`. Messages
//! marked with `DELE` are removed from the shared [`MockPop3Server`] maildrop
//! only when the client ends the session with `QUIT`, as the RFC requires.
//! Failures are injected per command with [`MockPop3Server::fail`] and
//! [`MockPop3Server::disconnect_on`].

use std::fmt::Write as _;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use md5::{Digest as _, Md5};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio_rustls::TlsAcceptor;

use nail_testkit::MockTls;
use nail_testkit::tls::TestTls;

use crate::MockNetError;
use crate::sasl;
use crate::stream::{Connection, MockStream, args, verb};

/// Configuration of a [`MockPop3Server`].
#[derive(Debug, Clone)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent feature switches of a mock server; each test turns single ones on or off"
)]
pub struct MockPop3Config {
    /// TLS mode. With [`MockTls::StartTls`], logging in requires `STLS`
    /// first.
    pub tls: MockTls,
    /// Accepted user name.
    pub username: String,
    /// Accepted password (`USER`/`PASS`, `APOP`, `AUTH PLAIN`).
    pub password: String,
    /// Put an APOP timestamp in the greeting and accept `APOP`.
    pub apop: bool,
    /// Accept `USER` / `PASS`.
    pub user_pass: bool,
    /// Advertised SASL mechanisms (`PLAIN`, `XOAUTH2`, `OAUTHBEARER`).
    pub sasl: Vec<String>,
    /// Accepted (user, access token) pairs for the OAuth mechanisms.
    pub oauth_tokens: Vec<(String, String)>,
    /// Support `CAPA`.
    pub capa: bool,
    /// Support `UIDL`.
    pub uidl: bool,
    /// Support `TOP`.
    pub top: bool,
}

impl Default for MockPop3Config {
    fn default() -> Self {
        Self {
            tls: MockTls::None,
            username: "sam@nail.example".to_owned(),
            password: "secret".to_owned(),
            apop: true,
            user_pass: true,
            sasl: vec!["PLAIN".to_owned(), "XOAUTH2".to_owned()],
            oauth_tokens: Vec::new(),
            capa: true,
            uidl: true,
            top: true,
        }
    }
}

/// A message in the mock maildrop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pop3Message {
    /// Unique id reported by `UIDL`.
    pub uid: String,
    /// Raw RFC 5322 message.
    pub data: Vec<u8>,
}

#[derive(Debug, Clone)]
enum Fault {
    Err(String),
    Disconnect,
}

#[derive(Debug, Default)]
struct Shared {
    maildrop: Mutex<Vec<Pop3Message>>,
    deleted: Mutex<Vec<String>>,
    faults: Mutex<Vec<(String, Fault, usize)>>,
    commands: Mutex<Vec<String>>,
    connections: AtomicUsize,
    logins: AtomicUsize,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Shared {
    fn take_fault(&self, command: &str) -> Option<Fault> {
        let mut faults = lock(&self.faults);
        let index = faults.iter().position(|(c, _, _)| c == command)?;
        let fault = faults[index].1.clone();
        faults[index].2 -= 1;
        if faults[index].2 == 0 {
            faults.remove(index);
        }
        Some(fault)
    }
}

/// A running mock POP3 server on `127.0.0.1`. Stops when dropped.
#[derive(Debug)]
pub struct MockPop3Server {
    addr: SocketAddr,
    ca_der: Option<Vec<u8>>,
    shared: Arc<Shared>,
    shutdown: watch::Sender<bool>,
    task: JoinHandle<()>,
}

impl MockPop3Server {
    /// Starts a server with the given maildrop on a free loopback port.
    pub async fn start(
        config: MockPop3Config,
        messages: Vec<Pop3Message>,
    ) -> Result<Self, MockNetError> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let addr = listener.local_addr()?;
        let tls = match config.tls {
            MockTls::None => None,
            MockTls::Implicit | MockTls::StartTls => Some(TestTls::generate()?),
        };
        let ca_der = tls.as_ref().map(|t| t.ca_der.clone());
        let acceptor = tls.map(|t| t.acceptor);
        let shared = Arc::new(Shared::default());
        *lock(&shared.maildrop) = messages;
        let (shutdown, mut stop) = watch::channel(false);
        let config = Arc::new(config);
        let task_shared = Arc::clone(&shared);
        let task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = stop.changed() => break,
                    accepted = listener.accept() => {
                        let Ok((tcp, _)) = accepted else { continue };
                        let n = task_shared.connections.fetch_add(1, Ordering::SeqCst);
                        let shared = Arc::clone(&task_shared);
                        let config = Arc::clone(&config);
                        let acceptor = acceptor.clone();
                        tokio::spawn(async move {
                            if let Err(err) = session(tcp, &shared, &config, acceptor.as_ref(), n).await {
                                tracing::debug!(%err, "mock POP3 session ended");
                            }
                        });
                    }
                }
            }
        });
        Ok(Self {
            addr,
            ca_der,
            shared,
            shutdown,
            task,
        })
    }

    /// The listening address.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The listening port.
    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    /// DER of the CA that signed the server certificate (TLS modes only).
    pub fn ca_der(&self) -> Option<&[u8]> {
        self.ca_der.as_deref()
    }

    /// Adds a message to the maildrop (visible to the next session).
    pub fn add_message(&self, uid: &str, data: impl Into<Vec<u8>>) {
        lock(&self.shared.maildrop).push(Pop3Message {
            uid: uid.to_owned(),
            data: data.into(),
        });
    }

    /// UIDs currently in the maildrop.
    pub fn uids(&self) -> Vec<String> {
        lock(&self.shared.maildrop)
            .iter()
            .map(|m| m.uid.clone())
            .collect()
    }

    /// UIDs deleted by committed sessions, in order.
    pub fn deleted(&self) -> Vec<String> {
        lock(&self.shared.deleted).clone()
    }

    /// Every command line received, in order (secrets removed).
    pub fn commands(&self) -> Vec<String> {
        lock(&self.shared.commands).clone()
    }

    /// Number of accepted TCP connections.
    pub fn connections(&self) -> usize {
        self.shared.connections.load(Ordering::SeqCst)
    }

    /// Number of successful logins.
    pub fn logins(&self) -> usize {
        self.shared.logins.load(Ordering::SeqCst)
    }

    /// Answers the next `times` occurrences of `command` with `-ERR text`.
    pub fn fail(&self, command: &str, text: &str, times: usize) {
        lock(&self.shared.faults).push((
            command.to_ascii_uppercase(),
            Fault::Err(text.to_owned()),
            times,
        ));
    }

    /// Drops the connection without answering the next `command`.
    pub fn disconnect_on(&self, command: &str) {
        lock(&self.shared.faults).push((command.to_ascii_uppercase(), Fault::Disconnect, 1));
    }
}

impl Drop for MockPop3Server {
    fn drop(&mut self) {
        let _ = self.shutdown.send(true);
        self.task.abort();
    }
}

/// The APOP digest: hex MD5 of the greeting timestamp and the password.
pub fn apop_digest(timestamp: &str, password: &str) -> String {
    let digest = Md5::digest(format!("{timestamp}{password}").as_bytes());
    digest.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

/// Encodes a message as a POP3 multi-line body: CRLF line ends,
/// dot-stuffing, terminating `.` line. With `lines = Some(n)`, only the
/// header and the first `n` body lines (`TOP`).
fn multiline(data: &[u8], top: Option<usize>) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 64);
    let mut in_body = false;
    let mut body_lines = 0usize;
    let text = data.strip_suffix(b"\n").unwrap_or(data);
    for line in text.split(|b| *b == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if in_body {
            if top.is_some_and(|n| body_lines >= n) {
                break;
            }
            body_lines += 1;
        }
        if line.first() == Some(&b'.') {
            out.push(b'.');
        }
        out.extend_from_slice(line);
        out.extend_from_slice(b"\r\n");
        if line.is_empty() {
            in_body = true;
        }
    }
    out.extend_from_slice(b".\r\n");
    out
}

struct Mailbox {
    messages: Vec<Pop3Message>,
    deleted: Vec<bool>,
}

impl Mailbox {
    fn get(&self, arg: &str) -> Result<(usize, &Pop3Message), &'static str> {
        let n: usize = arg
            .trim()
            .parse()
            .map_err(|_| "-ERR invalid message number")?;
        if n == 0 || n > self.messages.len() {
            return Err("-ERR no such message");
        }
        if self.deleted[n - 1] {
            return Err("-ERR message already deleted");
        }
        Ok((n, &self.messages[n - 1]))
    }

    fn live(&self) -> impl Iterator<Item = (usize, &Pop3Message)> {
        self.messages
            .iter()
            .enumerate()
            .filter(|(i, _)| !self.deleted[*i])
            .map(|(i, m)| (i + 1, m))
    }
}

async fn session(
    tcp: TcpStream,
    shared: &Shared,
    config: &MockPop3Config,
    acceptor: Option<&TlsAcceptor>,
    connection: usize,
) -> std::io::Result<()> {
    drop(tcp.set_nodelay(true));
    let mut conn = match (config.tls, acceptor) {
        (MockTls::Implicit, Some(acceptor)) => Connection::accept_tls(tcp, acceptor).await?,
        _ => Connection::new(MockStream::Plain(tcp)),
    };
    let timestamp = format!("<{}.{connection}@mock.nail.example>", std::process::id());
    if config.apop {
        conn.line(&format!("+OK Nail mock POP3 ready {timestamp}"))
            .await?;
    } else {
        conn.line("+OK Nail mock POP3 ready").await?;
    }
    let mut user: Option<String> = None;
    let mut mailbox: Option<Mailbox> = None;
    loop {
        let Some(raw) = conn.read_line().await? else {
            return Ok(());
        };
        let line = String::from_utf8_lossy(&raw).into_owned();
        let verb = verb(&line);
        let logged = match verb.as_str() {
            "PASS" => "PASS".to_owned(),
            "APOP" => format!(
                "APOP {}",
                args(&line).split_whitespace().next().unwrap_or_default()
            ),
            "AUTH" => format!(
                "AUTH {}",
                args(&line).split_whitespace().next().unwrap_or_default()
            ),
            _ => line.clone(),
        };
        lock(&shared.commands).push(logged);
        match shared.take_fault(&verb) {
            Some(Fault::Disconnect) => return Ok(()),
            Some(Fault::Err(text)) => {
                conn.line(&format!("-ERR {text}")).await?;
                continue;
            }
            None => {}
        }
        let tls_ok = config.tls != MockTls::StartTls || conn.is_tls();
        let Some(mb) = mailbox.as_mut() else {
            // AUTHORIZATION state.
            match verb.as_str() {
                "CAPA" if config.capa => {
                    let mut lines = vec!["+OK Capability list follows".to_owned()];
                    if config.top {
                        lines.push("TOP".to_owned());
                    }
                    if config.uidl {
                        lines.push("UIDL".to_owned());
                    }
                    if config.user_pass && tls_ok {
                        lines.push("USER".to_owned());
                    }
                    if !config.sasl.is_empty() && tls_ok {
                        lines.push(format!("SASL {}", config.sasl.join(" ")));
                    }
                    if config.tls == MockTls::StartTls && !conn.is_tls() {
                        lines.push("STLS".to_owned());
                    }
                    lines.push("RESP-CODES".to_owned());
                    lines.push("AUTH-RESP-CODE".to_owned());
                    lines.push("IMPLEMENTATION Nail-mock".to_owned());
                    lines.push(".".to_owned());
                    conn.write(format!("{}\r\n", lines.join("\r\n")).as_bytes())
                        .await?;
                }
                "STLS" => {
                    match acceptor.filter(|_| config.tls == MockTls::StartTls && !conn.is_tls()) {
                        Some(acceptor) => {
                            conn.line("+OK Begin TLS negotiation").await?;
                            conn = conn.upgrade(acceptor).await?;
                            user = None;
                        }
                        None => conn.line("-ERR STLS not available").await?,
                    }
                }
                "USER" if config.user_pass => {
                    if tls_ok {
                        user = Some(args(&line).to_owned());
                        conn.line("+OK").await?;
                    } else {
                        conn.line("-ERR [AUTH] TLS required").await?;
                    }
                }
                "PASS" if user.is_some() => {
                    let ok = user.as_deref() == Some(config.username.as_str())
                        && args(&line) == config.password;
                    if ok {
                        mailbox = Some(open(shared));
                        conn.line("+OK maildrop locked and ready").await?;
                    } else {
                        user = None;
                        conn.line("-ERR [AUTH] Invalid credentials").await?;
                    }
                }
                "APOP" if config.apop => {
                    let mut parts = args(&line).split_whitespace();
                    let (name, digest) = (
                        parts.next().unwrap_or_default(),
                        parts.next().unwrap_or_default(),
                    );
                    let ok = tls_ok
                        && name == config.username
                        && digest.eq_ignore_ascii_case(&apop_digest(&timestamp, &config.password));
                    if ok {
                        mailbox = Some(open(shared));
                        conn.line("+OK maildrop locked and ready").await?;
                    } else {
                        conn.line("-ERR [AUTH] Invalid credentials").await?;
                    }
                }
                "AUTH" => {
                    if !tls_ok {
                        conn.line("-ERR [AUTH] TLS required").await?;
                        continue;
                    }
                    if sasl_auth(&mut conn, config, args(&line)).await? {
                        mailbox = Some(open(shared));
                        conn.line("+OK maildrop locked and ready").await?;
                    }
                }
                "QUIT" => {
                    conn.line("+OK bye").await?;
                    conn.shutdown().await;
                    return Ok(());
                }
                _ => conn.line("-ERR command not valid in this state").await?,
            }
            continue;
        };
        // TRANSACTION state.
        let a = args(&line);
        match verb.as_str() {
            "STAT" => {
                let (count, size) = mb
                    .live()
                    .fold((0, 0), |(c, s), (_, m)| (c + 1, s + m.data.len()));
                conn.line(&format!("+OK {count} {size}")).await?;
            }
            "LIST" | "UIDL" if verb == "LIST" || config.uidl => {
                let value = |m: &Pop3Message| {
                    if verb == "LIST" {
                        m.data.len().to_string()
                    } else {
                        m.uid.clone()
                    }
                };
                if a.is_empty() {
                    let mut out = String::from("+OK\r\n");
                    for (n, m) in mb.live() {
                        let _ = write!(out, "{n} {}\r\n", value(m));
                    }
                    out.push_str(".\r\n");
                    conn.write(out.as_bytes()).await?;
                } else {
                    let reply = match mb.get(a) {
                        Ok((n, m)) => format!("+OK {n} {}", value(m)),
                        Err(e) => e.to_owned(),
                    };
                    conn.line(&reply).await?;
                }
            }
            "RETR" => match mb.get(a) {
                Ok((_, m)) => {
                    let mut out = format!("+OK {} octets\r\n", m.data.len()).into_bytes();
                    out.extend(multiline(&m.data, None));
                    conn.write(&out).await?;
                }
                Err(e) => conn.line(e).await?,
            },
            "TOP" if config.top => {
                let mut parts = a.split_whitespace();
                let n = parts.next().unwrap_or_default();
                let Some(lines) = parts.next().and_then(|l| l.parse::<usize>().ok()) else {
                    conn.line("-ERR syntax: TOP msg n").await?;
                    continue;
                };
                match mb.get(n) {
                    Ok((_, m)) => {
                        let mut out = b"+OK\r\n".to_vec();
                        out.extend(multiline(&m.data, Some(lines)));
                        conn.write(&out).await?;
                    }
                    Err(e) => conn.line(e).await?,
                }
            }
            "DELE" => {
                let reply = match mb.get(a) {
                    Ok((n, _)) => {
                        mb.deleted[n - 1] = true;
                        format!("+OK message {n} deleted")
                    }
                    Err(e) => e.to_owned(),
                };
                conn.line(&reply).await?;
            }
            "RSET" => {
                mb.deleted.iter_mut().for_each(|d| *d = false);
                conn.line("+OK").await?;
            }
            "NOOP" => conn.line("+OK").await?,
            "QUIT" => {
                let removed: Vec<String> = mb
                    .messages
                    .iter()
                    .zip(&mb.deleted)
                    .filter(|(_, d)| **d)
                    .map(|(m, _)| m.uid.clone())
                    .collect();
                lock(&shared.maildrop).retain(|m| !removed.contains(&m.uid));
                lock(&shared.deleted).extend(removed);
                conn.line("+OK bye").await?;
                conn.shutdown().await;
                return Ok(());
            }
            _ => conn.line("-ERR unknown command").await?,
        }
    }
}

fn open(shared: &Shared) -> Mailbox {
    shared.logins.fetch_add(1, Ordering::SeqCst);
    let messages = lock(&shared.maildrop).clone();
    let deleted = vec![false; messages.len()];
    Mailbox { messages, deleted }
}

/// Runs an `AUTH` exchange; `Ok(true)` when the client is logged in (the
/// caller sends the final `+OK`), `Ok(false)` after an `-ERR` was sent.
async fn sasl_auth(
    conn: &mut Connection,
    config: &MockPop3Config,
    arguments: &str,
) -> std::io::Result<bool> {
    let mut parts = arguments.split_whitespace();
    let mechanism = parts.next().unwrap_or_default().to_ascii_uppercase();
    if !config.sasl.contains(&mechanism) {
        conn.line("-ERR unsupported mechanism").await?;
        return Ok(false);
    }
    let payload = match parts.next() {
        Some("=") => String::new(),
        Some(initial) => initial.to_owned(),
        None => {
            conn.line("+ ").await?;
            let line = conn.read_line().await?.unwrap_or_default();
            String::from_utf8_lossy(&line).into_owned()
        }
    };
    if payload.trim() == "*" {
        conn.line("-ERR authentication cancelled").await?;
        return Ok(false);
    }
    let Some(decoded) = sasl::b64_decode(&payload) else {
        conn.line("-ERR invalid base64").await?;
        return Ok(false);
    };
    let ok = if mechanism.as_str() == "PLAIN" {
        sasl::parse_plain(&decoded)
            .is_some_and(|(u, p)| u == config.username && p == config.password)
    } else {
        let parsed = if mechanism == "XOAUTH2" {
            sasl::parse_xoauth2(&decoded)
        } else {
            sasl::parse_oauthbearer(&decoded)
        };
        let ok = parsed.is_some_and(|pair| config.oauth_tokens.contains(&pair));
        if !ok {
            conn.line(&format!("+ {}", sasl::oauth_error_challenge()))
                .await?;
            drop(conn.read_line().await?);
        }
        ok
    };
    if !ok {
        conn.line("-ERR [AUTH] Authentication failed").await?;
    }
    Ok(ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apop_digest_matches_rfc_1939_example() {
        assert_eq!(
            apop_digest("<1896.697170952@dbc.mtview.ca.us>", "tanstaaf"),
            "c4c9334bac560ecc979e58001b3e22fb"
        );
    }

    #[test]
    fn multiline_stuffs_dots_and_limits_top() {
        let msg = b"Subject: x\n\nline1\n.hidden\nline3\n";
        assert_eq!(
            multiline(msg, None),
            b"Subject: x\r\n\r\nline1\r\n..hidden\r\nline3\r\n.\r\n".to_vec()
        );
        assert_eq!(
            multiline(msg, Some(1)),
            b"Subject: x\r\n\r\nline1\r\n.\r\n".to_vec()
        );
        assert_eq!(multiline(msg, Some(0)), b"Subject: x\r\n\r\n.\r\n".to_vec());
    }
}
