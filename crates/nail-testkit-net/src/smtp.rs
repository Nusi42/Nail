//! An in-process mock SMTP submission server.
//!
//! Supported: `EHLO` / `HELO` (with `SIZE`, `8BITMIME`, `SMTPUTF8`, `DSN`,
//! `ENHANCEDSTATUSCODES`, `STARTTLS`, `AUTH`), `STARTTLS`, `AUTH PLAIN` /
//! `LOGIN` / `XOAUTH2` / `OAUTHBEARER` (with or without initial response,
//! including the JSON error challenge of the OAuth mechanisms), `MAIL`,
//! `RCPT`, `DATA`, `RSET`, `NOOP`, `QUIT`.
//!
//! Every accepted message is recorded as a [`ReceivedMail`]. Failures are
//! injected per command with [`MockSmtpServer::fail`],
//! [`MockSmtpServer::disconnect_on`] and
//! [`MockSmtpServer::reject_recipient`]. Command names for fault injection
//! are the SMTP verbs (`EHLO`, `AUTH`, `MAIL`, `RCPT`, `DATA`, …) plus
//! `CONNECT` (instead of the greeting) and `MESSAGE` (the reply after the
//! message data).

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Notify, watch};
use tokio::task::JoinHandle;
use tokio_rustls::TlsAcceptor;

use nail_testkit::MockTls;
use nail_testkit::tls::TestTls;

use crate::MockNetError;
use crate::sasl;
use crate::stream::{Connection, MockStream, args, verb};

/// Configuration of a [`MockSmtpServer`].
#[derive(Debug, Clone)]
pub struct MockSmtpConfig {
    /// Host name in the greeting and `EHLO` reply.
    pub hostname: String,
    /// TLS mode. With [`MockTls::StartTls`], `AUTH` is only offered and
    /// accepted after `STARTTLS`.
    pub tls: MockTls,
    /// Accepted (user, password) pairs for `PLAIN` / `LOGIN`.
    pub users: Vec<(String, String)>,
    /// Accepted (user, access token) pairs for `XOAUTH2` / `OAUTHBEARER`;
    /// more can be added at run time with [`MockSmtpServer::add_oauth_token`].
    pub oauth_tokens: Vec<(String, String)>,
    /// Advertised SASL mechanisms (upper case).
    pub mechanisms: Vec<String>,
    /// Whether `MAIL` requires a successful `AUTH` first.
    pub require_auth: bool,
    /// The `SIZE` limit in bytes; `None` advertises `SIZE` without a limit.
    pub size_limit: Option<usize>,
    /// Advertise `8BITMIME`.
    pub eight_bit_mime: bool,
    /// Advertise `SMTPUTF8`.
    pub smtputf8: bool,
    /// Advertise `DSN`.
    pub dsn: bool,
}

impl Default for MockSmtpConfig {
    fn default() -> Self {
        Self {
            hostname: "mock.nail.example".to_owned(),
            tls: MockTls::None,
            users: vec![("sam@nail.example".to_owned(), "secret".to_owned())],
            oauth_tokens: Vec::new(),
            mechanisms: ["PLAIN", "LOGIN", "XOAUTH2", "OAUTHBEARER"]
                .map(str::to_owned)
                .to_vec(),
            require_auth: true,
            size_limit: Some(10 * 1024 * 1024),
            eight_bit_mime: true,
            smtputf8: true,
            dsn: true,
        }
    }
}

/// One `RCPT TO` of a received message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recipient {
    /// The address between `<` and `>`.
    pub address: String,
    /// ESMTP parameters (`NOTIFY=FAILURE`, …) as sent.
    pub params: Vec<String>,
}

/// A message the server accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceivedMail {
    /// `MAIL FROM` address (empty for the null sender `<>`).
    pub mail_from: String,
    /// `MAIL FROM` parameters (`SIZE=…`, `BODY=8BITMIME`, `RET=HDRS`, …).
    pub mail_params: Vec<String>,
    /// Accepted recipients in order.
    pub recipients: Vec<Recipient>,
    /// The message as transferred, dot-stuffing removed, CRLF line ends.
    pub data: Vec<u8>,
    /// The authenticated user, if any.
    pub authenticated_as: Option<String>,
    /// The SASL mechanism used, if any.
    pub mechanism: Option<String>,
    /// Whether the session was encrypted.
    pub tls: bool,
}

impl ReceivedMail {
    /// Recipient addresses only.
    pub fn recipient_addresses(&self) -> Vec<&str> {
        self.recipients.iter().map(|r| r.address.as_str()).collect()
    }

    /// The message as (lossy) UTF-8 text.
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.data).into_owned()
    }
}

#[derive(Debug, Clone)]
enum Fault {
    Reply(u16, String),
    Disconnect,
}

#[derive(Debug)]
struct FaultRule {
    command: String,
    fault: Fault,
    /// `None` = forever.
    remaining: Option<usize>,
}

#[derive(Debug, Default)]
struct Shared {
    faults: Mutex<Vec<FaultRule>>,
    rejected: Mutex<HashMap<String, (u16, String)>>,
    oauth_tokens: Mutex<Vec<(String, String)>>,
    messages: Mutex<Vec<ReceivedMail>>,
    commands: Mutex<Vec<String>>,
    connections: AtomicUsize,
    received: Notify,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Shared {
    fn take_fault(&self, command: &str) -> Option<Fault> {
        let mut faults = lock(&self.faults);
        let index = faults.iter().position(|f| f.command == command)?;
        let fault = faults[index].fault.clone();
        match &mut faults[index].remaining {
            Some(n) if *n <= 1 => {
                faults.remove(index);
            }
            Some(n) => *n -= 1,
            None => {}
        }
        Some(fault)
    }

    fn log(&self, line: &str) {
        let redacted = if verb(line) == "AUTH" {
            let mech = args(line).split_whitespace().next().unwrap_or_default();
            format!("AUTH {mech}")
        } else {
            line.to_owned()
        };
        lock(&self.commands).push(redacted);
    }
}

/// A running mock SMTP server on `127.0.0.1`. Stops when dropped.
#[derive(Debug)]
pub struct MockSmtpServer {
    addr: SocketAddr,
    ca_der: Option<Vec<u8>>,
    shared: Arc<Shared>,
    shutdown: watch::Sender<bool>,
    task: JoinHandle<()>,
}

impl MockSmtpServer {
    /// Starts a server on a free loopback port.
    pub async fn start(config: MockSmtpConfig) -> Result<Self, MockNetError> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let addr = listener.local_addr()?;
        let tls = match config.tls {
            MockTls::None => None,
            MockTls::Implicit | MockTls::StartTls => Some(TestTls::generate()?),
        };
        let ca_der = tls.as_ref().map(|t| t.ca_der.clone());
        let acceptor = tls.map(|t| t.acceptor);
        let shared = Arc::new(Shared::default());
        lock(&shared.oauth_tokens).extend(config.oauth_tokens.iter().cloned());
        let (shutdown, mut stop) = watch::channel(false);
        let config = Arc::new(config);
        let task_shared = Arc::clone(&shared);
        let task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = stop.changed() => break,
                    accepted = listener.accept() => {
                        let Ok((tcp, _)) = accepted else { continue };
                        task_shared.connections.fetch_add(1, Ordering::SeqCst);
                        let shared = Arc::clone(&task_shared);
                        let config = Arc::clone(&config);
                        let acceptor = acceptor.clone();
                        tokio::spawn(async move {
                            if let Err(err) = session(tcp, &shared, &config, acceptor.as_ref()).await {
                                tracing::debug!(%err, "mock SMTP session ended");
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

    /// Messages accepted so far.
    pub fn messages(&self) -> Vec<ReceivedMail> {
        lock(&self.shared.messages).clone()
    }

    /// Waits until at least `count` messages were accepted; returns them, or
    /// `None` after `timeout`.
    pub async fn wait_for_messages(
        &self,
        count: usize,
        timeout: Duration,
    ) -> Option<Vec<ReceivedMail>> {
        let wait = async {
            loop {
                let notified = self.shared.received.notified();
                {
                    let messages = lock(&self.shared.messages);
                    if messages.len() >= count {
                        return messages.clone();
                    }
                }
                notified.await;
            }
        };
        tokio::time::timeout(timeout, wait).await.ok()
    }

    /// Every command line received, in order (`AUTH` payloads removed).
    pub fn commands(&self) -> Vec<String> {
        lock(&self.shared.commands).clone()
    }

    /// Number of accepted TCP connections.
    pub fn connections(&self) -> usize {
        self.shared.connections.load(Ordering::SeqCst)
    }

    /// Answers the next `times` occurrences of `command` with `code text`
    /// instead of processing it (`None` = every time).
    pub fn fail(&self, command: &str, code: u16, text: &str, times: Option<usize>) {
        lock(&self.shared.faults).push(FaultRule {
            command: command.to_ascii_uppercase(),
            fault: Fault::Reply(code, text.to_owned()),
            remaining: times,
        });
    }

    /// Drops the connection without answering the next `command`.
    pub fn disconnect_on(&self, command: &str) {
        lock(&self.shared.faults).push(FaultRule {
            command: command.to_ascii_uppercase(),
            fault: Fault::Disconnect,
            remaining: Some(1),
        });
    }

    /// Answers `RCPT TO:<address>` with `code text` (case-insensitive).
    pub fn reject_recipient(&self, address: &str, code: u16, text: &str) {
        lock(&self.shared.rejected).insert(address.to_ascii_lowercase(), (code, text.to_owned()));
    }

    /// Accepts `token` for `user` with `XOAUTH2` / `OAUTHBEARER` from now on.
    pub fn add_oauth_token(&self, user: &str, token: &str) {
        lock(&self.shared.oauth_tokens).push((user.to_owned(), token.to_owned()));
    }
}

impl Drop for MockSmtpServer {
    fn drop(&mut self) {
        let _ = self.shutdown.send(true);
        self.task.abort();
    }
}

#[derive(Debug, Default)]
struct Transaction {
    from: Option<(String, Vec<String>)>,
    recipients: Vec<Recipient>,
}

#[derive(Debug, Default)]
struct SessionState {
    greeted: bool,
    user: Option<String>,
    mechanism: Option<String>,
    txn: Transaction,
}

async fn session(
    tcp: TcpStream,
    shared: &Shared,
    config: &MockSmtpConfig,
    acceptor: Option<&TlsAcceptor>,
) -> std::io::Result<()> {
    let _ = tcp.set_nodelay(true);
    let mut conn = match (config.tls, acceptor) {
        (MockTls::Implicit, Some(acceptor)) => Connection::accept_tls(tcp, acceptor).await?,
        _ => Connection::new(MockStream::Plain(tcp)),
    };
    match shared.take_fault("CONNECT") {
        Some(Fault::Reply(code, text)) => {
            conn.line(&format!("{code} {text}")).await?;
            conn.shutdown().await;
            return Ok(());
        }
        Some(Fault::Disconnect) => return Ok(()),
        None => {}
    }
    conn.line(&format!("220 {} ESMTP Nail mock ready", config.hostname))
        .await?;
    let mut st = SessionState::default();
    loop {
        let Some(raw) = conn.read_line().await? else {
            return Ok(());
        };
        let line = String::from_utf8_lossy(&raw).into_owned();
        let verb = verb(&line);
        shared.log(&line);
        match shared.take_fault(&verb) {
            Some(Fault::Disconnect) => return Ok(()),
            Some(Fault::Reply(code, text)) => {
                conn.line(&format!("{code} {text}")).await?;
                if verb == "QUIT" {
                    return Ok(());
                }
                continue;
            }
            None => {}
        }
        match verb.as_str() {
            "EHLO" | "HELO" => {
                st.greeted = true;
                st.txn = Transaction::default();
                let lines = ehlo_lines(config, &conn, args(&line), verb == "HELO");
                conn.write(lines.as_bytes()).await?;
            }
            "STARTTLS" => {
                let Some(acceptor) = acceptor.filter(|_| config.tls == MockTls::StartTls) else {
                    conn.line("502 5.5.1 STARTTLS not available").await?;
                    continue;
                };
                if conn.is_tls() {
                    conn.line("503 5.5.1 TLS already active").await?;
                    continue;
                }
                conn.line("220 2.0.0 Ready to start TLS").await?;
                conn = conn.upgrade(acceptor).await?;
                st = SessionState::default();
            }
            "AUTH" => auth(&mut conn, shared, config, &mut st, args(&line)).await?,
            "MAIL" => {
                let reply = mail(shared, config, &conn, &mut st, args(&line));
                conn.line(&reply).await?;
            }
            "RCPT" => {
                let reply = rcpt(shared, &mut st, args(&line));
                conn.line(&reply).await?;
            }
            "DATA" => data(&mut conn, shared, config, &mut st).await?,
            "RSET" => {
                st.txn = Transaction::default();
                conn.line("250 2.0.0 Ok").await?;
            }
            "NOOP" => conn.line("250 2.0.0 Ok").await?,
            "QUIT" => {
                conn.line("221 2.0.0 Bye").await?;
                conn.shutdown().await;
                return Ok(());
            }
            _ => conn.line("502 5.5.2 Command not recognized").await?,
        }
    }
}

fn auth_offered(config: &MockSmtpConfig, conn: &Connection) -> bool {
    !config.mechanisms.is_empty() && (config.tls != MockTls::StartTls || conn.is_tls())
}

fn ehlo_lines(config: &MockSmtpConfig, conn: &Connection, client: &str, helo: bool) -> String {
    if helo {
        return format!("250 {} greets {client}\r\n", config.hostname);
    }
    let mut lines = vec![format!("{} greets {client}", config.hostname)];
    lines.push(match config.size_limit {
        Some(limit) => format!("SIZE {limit}"),
        None => "SIZE".to_owned(),
    });
    if config.eight_bit_mime {
        lines.push("8BITMIME".to_owned());
    }
    if config.smtputf8 {
        lines.push("SMTPUTF8".to_owned());
    }
    if config.dsn {
        lines.push("DSN".to_owned());
    }
    lines.push("ENHANCEDSTATUSCODES".to_owned());
    if config.tls == MockTls::StartTls && !conn.is_tls() {
        lines.push("STARTTLS".to_owned());
    }
    if auth_offered(config, conn) {
        lines.push(format!("AUTH {}", config.mechanisms.join(" ")));
    }
    lines.push("HELP".to_owned());
    let last = lines.len() - 1;
    lines
        .iter()
        .enumerate()
        .map(|(i, l)| format!("250{}{l}\r\n", if i == last { ' ' } else { '-' }))
        .collect()
}

/// Reads one SASL continuation line; `None` when the client cancelled (`*`).
async fn continuation(conn: &mut Connection, challenge: &str) -> std::io::Result<Option<String>> {
    conn.line(&format!("334 {challenge}")).await?;
    let line = conn.read_line().await?.unwrap_or_default();
    let line = String::from_utf8_lossy(&line).into_owned();
    Ok((line.trim() != "*").then_some(line))
}

async fn auth(
    conn: &mut Connection,
    shared: &Shared,
    config: &MockSmtpConfig,
    st: &mut SessionState,
    arguments: &str,
) -> std::io::Result<()> {
    if config.tls == MockTls::StartTls && !conn.is_tls() {
        return conn
            .line("530 5.7.0 Must issue a STARTTLS command first")
            .await;
    }
    if st.user.is_some() {
        return conn.line("503 5.5.1 Already authenticated").await;
    }
    let mut parts = arguments.split_whitespace();
    let mechanism = parts.next().unwrap_or_default().to_ascii_uppercase();
    let initial = parts.next().map(|i| {
        if i == "=" {
            String::new()
        } else {
            i.to_owned()
        }
    });
    if !auth_offered(config, conn) || !config.mechanisms.contains(&mechanism) {
        return conn
            .line("504 5.5.4 Unrecognized authentication type")
            .await;
    }
    const CANCELLED: &str = "501 5.0.0 Authentication cancelled";
    const INVALID: &str = "535 5.7.8 Authentication credentials invalid";
    let user = match mechanism.as_str() {
        "PLAIN" => {
            let payload = match initial {
                Some(p) => p,
                None => match continuation(conn, "").await? {
                    Some(p) => p,
                    None => return conn.line(CANCELLED).await,
                },
            };
            let Some((user, pass)) = sasl::b64_decode(&payload).and_then(|p| sasl::parse_plain(&p))
            else {
                return conn.line("501 5.5.2 Cannot decode response").await;
            };
            config
                .users
                .iter()
                .any(|(u, p)| *u == user && *p == pass)
                .then_some(user)
        }
        "LOGIN" => {
            let user = match initial {
                Some(u) => u,
                None => match continuation(conn, "VXNlcm5hbWU6").await? {
                    Some(u) => u,
                    None => return conn.line(CANCELLED).await,
                },
            };
            let Some(pass) = continuation(conn, "UGFzc3dvcmQ6").await? else {
                return conn.line(CANCELLED).await;
            };
            let decode = |s: &str| sasl::b64_decode(s).and_then(|b| String::from_utf8(b).ok());
            let (Some(user), Some(pass)) = (decode(&user), decode(&pass)) else {
                return conn.line("501 5.5.2 Cannot decode response").await;
            };
            config
                .users
                .iter()
                .any(|(u, p)| *u == user && *p == pass)
                .then_some(user)
        }
        "XOAUTH2" | "OAUTHBEARER" => {
            let payload = match initial {
                Some(p) => p,
                None => match continuation(conn, "").await? {
                    Some(p) => p,
                    None => return conn.line(CANCELLED).await,
                },
            };
            let parsed = sasl::b64_decode(&payload).and_then(|p| {
                if mechanism == "XOAUTH2" {
                    sasl::parse_xoauth2(&p)
                } else {
                    sasl::parse_oauthbearer(&p)
                }
            });
            let valid = parsed.filter(|(user, token)| {
                lock(&shared.oauth_tokens)
                    .iter()
                    .any(|(u, t)| u == user && t == token)
            });
            match valid {
                Some((user, _)) => Some(user),
                None => {
                    // RFC 7628 / XOAUTH2: an error challenge, the client
                    // answers with a dummy response, then the final failure.
                    conn.line(&format!("334 {}", sasl::oauth_error_challenge()))
                        .await?;
                    let _ = conn.read_line().await?;
                    return conn
                        .line("535 5.7.8 Username and Password not accepted")
                        .await;
                }
            }
        }
        _ => {
            return conn
                .line("504 5.5.4 Unrecognized authentication type")
                .await;
        }
    };
    match user {
        Some(user) => {
            st.user = Some(user);
            st.mechanism = Some(mechanism);
            conn.line("235 2.7.0 Authentication successful").await
        }
        None => conn.line(INVALID).await,
    }
}

/// Parses `FROM:<addr> PARAM…` / `TO:<addr> PARAM…`.
fn parse_path(arguments: &str, keyword: &str) -> Option<(String, Vec<String>)> {
    let rest = arguments.trim();
    if rest.len() < keyword.len() || !rest[..keyword.len()].eq_ignore_ascii_case(keyword) {
        return None;
    }
    let rest = rest[keyword.len()..].trim_start();
    let rest = rest.strip_prefix('<')?;
    let (address, params) = rest.split_once('>')?;
    Some((
        address.to_owned(),
        params.split_whitespace().map(str::to_owned).collect(),
    ))
}

fn mail(
    shared: &Shared,
    config: &MockSmtpConfig,
    conn: &Connection,
    st: &mut SessionState,
    arguments: &str,
) -> String {
    let _ = shared;
    if !st.greeted {
        return "503 5.5.1 Send EHLO first".to_owned();
    }
    if config.require_auth && st.user.is_none() {
        return "530 5.7.0 Authentication required".to_owned();
    }
    if config.tls == MockTls::StartTls && !conn.is_tls() && config.require_auth {
        return "530 5.7.0 Must issue a STARTTLS command first".to_owned();
    }
    if st.txn.from.is_some() {
        return "503 5.5.1 Nested MAIL command".to_owned();
    }
    let Some((from, params)) = parse_path(arguments, "FROM:") else {
        return "501 5.5.4 Syntax: MAIL FROM:<address>".to_owned();
    };
    if let Some(limit) = config.size_limit {
        let declared = params.iter().find_map(|p| {
            p.split_once('=')
                .filter(|(k, _)| k.eq_ignore_ascii_case("SIZE"))
                .and_then(|(_, v)| v.parse::<usize>().ok())
        });
        if declared.is_some_and(|size| size > limit) {
            return "552 5.3.4 Message size exceeds fixed maximum message size".to_owned();
        }
    }
    st.txn.from = Some((from, params));
    "250 2.1.0 Ok".to_owned()
}

fn rcpt(shared: &Shared, st: &mut SessionState, arguments: &str) -> String {
    if st.txn.from.is_none() {
        return "503 5.5.1 Need MAIL command".to_owned();
    }
    let Some((address, params)) = parse_path(arguments, "TO:") else {
        return "501 5.5.4 Syntax: RCPT TO:<address>".to_owned();
    };
    if let Some((code, text)) = lock(&shared.rejected).get(&address.to_ascii_lowercase()) {
        return format!("{code} {text}");
    }
    st.txn.recipients.push(Recipient { address, params });
    "250 2.1.5 Ok".to_owned()
}

async fn data(
    conn: &mut Connection,
    shared: &Shared,
    config: &MockSmtpConfig,
    st: &mut SessionState,
) -> std::io::Result<()> {
    if st.txn.recipients.is_empty() {
        return conn.line("503 5.5.1 Need RCPT command").await;
    }
    conn.line("354 End data with <CR><LF>.<CR><LF>").await?;
    let limit = config.size_limit.unwrap_or(usize::MAX);
    let block = conn.read_dot_block(limit).await?;
    let txn = std::mem::take(&mut st.txn);
    let data = match block {
        Ok(data) => data,
        Err(_) => {
            return conn
                .line("552 5.3.4 Message size exceeds fixed maximum message size")
                .await;
        }
    };
    match shared.take_fault("MESSAGE") {
        Some(Fault::Reply(code, text)) => return conn.line(&format!("{code} {text}")).await,
        Some(Fault::Disconnect) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionAborted,
                "fault injected",
            ));
        }
        None => {}
    }
    let (mail_from, mail_params) = txn.from.unwrap_or_default();
    let id = {
        let mut messages = lock(&shared.messages);
        messages.push(ReceivedMail {
            mail_from,
            mail_params,
            recipients: txn.recipients,
            data,
            authenticated_as: st.user.clone(),
            mechanism: st.mechanism.clone(),
            tls: conn.is_tls(),
        });
        messages.len()
    };
    shared.received.notify_waiters();
    conn.line(&format!("250 2.0.0 Ok: queued as MOCK{id}"))
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    /// A tiny line client for driving the server by hand.
    struct Client {
        reader: BufReader<TcpStream>,
    }

    impl Client {
        async fn connect(addr: SocketAddr) -> Self {
            let tcp = TcpStream::connect(addr).await.unwrap();
            Self {
                reader: BufReader::new(tcp),
            }
        }

        /// Reads one (possibly multi-line) reply; returns all lines.
        async fn reply(&mut self) -> Vec<String> {
            let mut lines = Vec::new();
            loop {
                let mut line = String::new();
                self.reader.read_line(&mut line).await.unwrap();
                let line = line.trim_end().to_owned();
                let done = line.len() < 4 || line.as_bytes()[3] == b' ';
                lines.push(line);
                if done {
                    return lines;
                }
            }
        }

        async fn send(&mut self, line: &str) -> Vec<String> {
            self.reader
                .get_mut()
                .write_all(format!("{line}\r\n").as_bytes())
                .await
                .unwrap();
            self.reply().await
        }
    }

    fn plain(user: &str, pass: &str) -> String {
        sasl::b64_encode(format!("\0{user}\0{pass}").as_bytes())
    }

    #[tokio::test]
    async fn accepts_and_records_a_message() {
        let server = MockSmtpServer::start(MockSmtpConfig::default())
            .await
            .unwrap();
        let mut c = Client::connect(server.addr()).await;
        assert!(c.reply().await[0].starts_with("220 "));
        let ehlo = c.send("EHLO test").await;
        assert!(ehlo.iter().any(|l| l == "250-SIZE 10485760"));
        assert!(
            ehlo.iter()
                .any(|l| l.contains("AUTH PLAIN LOGIN XOAUTH2 OAUTHBEARER"))
        );
        assert!(c.send("MAIL FROM:<a@x>").await[0].starts_with("530"));
        assert!(
            c.send(&format!("AUTH PLAIN {}", plain("sam@nail.example", "nope")))
                .await[0]
                .starts_with("535")
        );
        assert!(
            c.send(&format!(
                "AUTH PLAIN {}",
                plain("sam@nail.example", "secret")
            ))
            .await[0]
                .starts_with("235")
        );
        assert!(c.send("MAIL FROM:<a@x> SIZE=20 RET=HDRS").await[0].starts_with("250"));
        assert!(c.send("RCPT TO:<b@y> NOTIFY=FAILURE").await[0].starts_with("250"));
        assert!(c.send("DATA").await[0].starts_with("354"));
        let reply = c.send("Subject: hi\r\n\r\n..dot\r\n.").await;
        assert!(
            reply[0].starts_with("250 2.0.0 Ok: queued as MOCK1"),
            "{reply:?}"
        );
        assert!(c.send("QUIT").await[0].starts_with("221"));

        let mail = server
            .wait_for_messages(1, Duration::from_secs(5))
            .await
            .unwrap()
            .remove(0);
        assert_eq!(mail.mail_from, "a@x");
        assert_eq!(mail.mail_params, ["SIZE=20", "RET=HDRS"]);
        assert_eq!(mail.recipient_addresses(), ["b@y"]);
        assert_eq!(mail.recipients[0].params, ["NOTIFY=FAILURE"]);
        assert_eq!(mail.text(), "Subject: hi\r\n\r\n.dot\r\n");
        assert_eq!(mail.authenticated_as.as_deref(), Some("sam@nail.example"));
        assert_eq!(mail.mechanism.as_deref(), Some("PLAIN"));
        assert!(!mail.tls);
        let commands = server.commands();
        assert!(commands.contains(&"AUTH PLAIN".to_owned()));
        assert!(
            !commands
                .iter()
                .any(|c| c.contains(&plain("sam@nail.example", "secret")))
        );
        assert_eq!(server.connections(), 1);
    }

    #[tokio::test]
    async fn login_and_oauth_mechanisms() {
        let server = MockSmtpServer::start(MockSmtpConfig {
            oauth_tokens: vec![("sam@nail.example".into(), "tok".into())],
            ..MockSmtpConfig::default()
        })
        .await
        .unwrap();
        let mut c = Client::connect(server.addr()).await;
        c.reply().await;
        c.send("EHLO t").await;
        assert_eq!(c.send("AUTH LOGIN").await[0], "334 VXNlcm5hbWU6");
        assert_eq!(
            c.send(&sasl::b64_encode(b"sam@nail.example")).await[0],
            "334 UGFzc3dvcmQ6"
        );
        assert!(c.send(&sasl::b64_encode(b"secret")).await[0].starts_with("235"));

        let mut c = Client::connect(server.addr()).await;
        c.reply().await;
        c.send("EHLO t").await;
        let bad = sasl::b64_encode(b"user=sam@nail.example\x01auth=Bearer bad\x01\x01");
        assert!(c.send(&format!("AUTH XOAUTH2 {bad}")).await[0].starts_with("334 "));
        assert!(c.send("").await[0].starts_with("535"));
        server.add_oauth_token("sam@nail.example", "tok2");
        let good = sasl::b64_encode(b"n,a=sam@nail.example,\x01auth=Bearer tok2\x01\x01");
        assert!(c.send(&format!("AUTH OAUTHBEARER {good}")).await[0].starts_with("235"));
    }

    #[tokio::test]
    async fn faults_and_limits() {
        let server = MockSmtpServer::start(MockSmtpConfig {
            require_auth: false,
            size_limit: Some(10),
            ..MockSmtpConfig::default()
        })
        .await
        .unwrap();
        server.fail("MAIL", 451, "4.3.0 Try later", Some(1));
        server.reject_recipient("Nobody@Y", 550, "5.1.1 No such user");
        let mut c = Client::connect(server.addr()).await;
        c.reply().await;
        c.send("EHLO t").await;
        assert_eq!(c.send("MAIL FROM:<a@x>").await[0], "451 4.3.0 Try later");
        assert!(c.send("MAIL FROM:<a@x> SIZE=11").await[0].starts_with("552"));
        assert!(c.send("MAIL FROM:<a@x>").await[0].starts_with("250"));
        assert_eq!(
            c.send("RCPT TO:<nobody@y>").await[0],
            "550 5.1.1 No such user"
        );
        assert!(c.send("RCPT TO:<b@y>").await[0].starts_with("250"));
        assert!(c.send("DATA").await[0].starts_with("354"));
        assert!(c.send("0123456789abc\r\n.").await[0].starts_with("552"));
        assert!(server.messages().is_empty());

        server.fail("CONNECT", 554, "5.3.2 No service", Some(1));
        let mut c = Client::connect(server.addr()).await;
        assert_eq!(c.reply().await[0], "554 5.3.2 No service");
    }
}
