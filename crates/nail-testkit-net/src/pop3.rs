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
use std::sync::{Arc, Mutex};

use md5::{Digest as _, Md5};

use nail_testkit::MockTls;

use crate::MockNetError;
use crate::sasl;
use crate::server::{Journal, MockListener, Session, journal_controls, lock, login_controls};
use crate::stream::{Connection, args};

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
            username: crate::MOCK_USER.to_owned(),
            password: crate::MOCK_PASSWORD.to_owned(),
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
    journal: Journal<Fault>,
}

/// A running mock POP3 server on `127.0.0.1`. Stops when dropped.
#[derive(Debug)]
pub struct MockPop3Server {
    listener: MockListener,
    shared: Arc<Shared>,
}

/// The address, port, CA and connection count of the listener.
impl std::ops::Deref for MockPop3Server {
    type Target = MockListener;

    fn deref(&self) -> &MockListener {
        &self.listener
    }
}

impl MockPop3Server {
    /// Starts a server with the given maildrop on a free loopback port.
    pub async fn start(
        config: MockPop3Config,
        messages: Vec<Pop3Message>,
    ) -> Result<Self, MockNetError> {
        let shared = Arc::new(Shared::default());
        *lock(&shared.maildrop) = messages;
        let listener = MockListener::serve(config.tls, "POP3", &shared, config, session).await?;
        Ok(Self { listener, shared })
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
}

journal_controls!(MockPop3Server);
login_controls!(MockPop3Server, Fault::Err, "`-ERR text`");

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
    mut conn: Connection,
    session: Session<Shared, MockPop3Config>,
) -> std::io::Result<()> {
    let (shared, config, acceptor) = session.parts();
    let timestamp = format!(
        "<{}.{}@mock.nail.example>",
        std::process::id(),
        session.number
    );
    if config.apop {
        conn.line(&format!("+OK Nail mock POP3 ready {timestamp}"))
            .await?;
    } else {
        conn.line("+OK Nail mock POP3 ready").await?;
    }
    let mut user: Option<String> = None;
    let mut mailbox: Option<Mailbox> = None;
    loop {
        let Some((line, verb)) = conn.next_command().await? else {
            return Ok(());
        };
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
        shared.journal.command(logged);
        match shared.journal.faults.take(&verb) {
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
    shared.journal.login();
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
    sasl::finish(
        conn,
        &mechanism,
        &payload,
        sasl::login_of!(config),
        &ANSWERS,
    )
    .await
}

/// POP3's answers to a failed `AUTH`.
const ANSWERS: sasl::Answers = sasl::Answers {
    cancelled: "-ERR authentication cancelled",
    bad_base64: "-ERR invalid base64",
    failed: "-ERR [AUTH] Authentication failed",
    challenge: |text| format!("+ {text}"),
};

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
