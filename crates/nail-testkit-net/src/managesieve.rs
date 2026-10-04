//! An in-process mock ManageSieve server (RFC 5804), M2c WP51.
//!
//! Like the other mocks here it binds to `127.0.0.1:0` and stops when
//! dropped. Supported: the capability greeting (`IMPLEMENTATION`, `SASL`,
//! `SIEVE`, `STARTTLS`, `MAXREDIRECTS`, `VERSION`), `CAPABILITY`,
//! `STARTTLS` (capabilities re-issued after the handshake), `AUTHENTICATE`
//! `PLAIN` / `XOAUTH2` / `OAUTHBEARER` (with or without initial response,
//! `"*"` cancels), `LISTSCRIPTS`, `PUTSCRIPT`, `CHECKSCRIPT`, `GETSCRIPT`,
//! `SETACTIVE`, `DELETESCRIPT`, `RENAMESCRIPT`, `HAVESPACE`, `NOOP`,
//! `LOGOUT`. Client strings may be quoted or literals (`{n+}` / `{n}`).
//!
//! Scripts are checked minimally by [`validate_script`] (lexical syntax,
//! bracket balance, `require` against the advertised `SIEVE` extensions,
//! extension commands used without `require`). Every stored script, every
//! upload and every `CHECKSCRIPT` is recorded so tests (and the demo-multi
//! seed, WP55) can assert what landed on the server. Failures are injected
//! with [`MockSieveServer::fail`], [`MockSieveServer::disconnect_on`] and
//! [`MockSieveServer::reject_scripts_containing`].

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio_rustls::TlsAcceptor;

use nail_testkit::MockTls;
use nail_testkit::tls::TestTls;

use crate::MockNetError;
use crate::sasl;
use crate::stream::{Connection, MockStream};

/// The extensions a [`MockSieveServer`] advertises by default (roughly
/// Dovecot Pigeonhole's default set).
pub const DEFAULT_EXTENSIONS: &[&str] = &[
    "fileinto",
    "reject",
    "envelope",
    "encoded-character",
    "vacation",
    "subaddress",
    "comparator-i;ascii-numeric",
    "comparator-i;unicode-casemap",
    "relational",
    "regex",
    "imap4flags",
    "copy",
    "include",
    "variables",
    "body",
    "mailbox",
    "date",
    "index",
];

/// Configuration of a [`MockSieveServer`].
#[derive(Debug, Clone)]
pub struct MockSieveConfig {
    /// TLS mode. With [`MockTls::StartTls`], `AUTHENTICATE` requires
    /// `STARTTLS` first and no SASL mechanism is advertised before it.
    pub tls: MockTls,
    /// Accepted user name.
    pub username: String,
    /// Accepted password (`PLAIN`).
    pub password: String,
    /// Advertised SASL mechanisms (`PLAIN`, `XOAUTH2`, `OAUTHBEARER`).
    pub sasl: Vec<String>,
    /// Accepted (user, access token) pairs for the OAuth mechanisms.
    pub oauth_tokens: Vec<(String, String)>,
    /// The `SIEVE` capability (lowercase extension names).
    pub extensions: Vec<String>,
    /// The `IMPLEMENTATION` capability.
    pub implementation: String,
    /// Advertise `VERSION "1.0"` (RFC 5804 servers; enables `CHECKSCRIPT`,
    /// `RENAMESCRIPT` and `NOOP`).
    pub version: bool,
    /// The `MAXREDIRECTS` capability.
    pub max_redirects: Option<u32>,
    /// Largest script accepted (`NO (QUOTA/MAXSIZE)` above).
    pub max_script_size: Option<usize>,
    /// Most scripts stored (`NO (QUOTA/MAXSCRIPTS)` above).
    pub max_scripts: Option<usize>,
}

impl Default for MockSieveConfig {
    fn default() -> Self {
        Self {
            tls: MockTls::None,
            username: "sam@nail.example".to_owned(),
            password: "secret".to_owned(),
            sasl: vec![
                "PLAIN".to_owned(),
                "XOAUTH2".to_owned(),
                "OAUTHBEARER".to_owned(),
            ],
            oauth_tokens: Vec::new(),
            extensions: DEFAULT_EXTENSIONS.iter().map(|e| (*e).to_owned()).collect(),
            implementation: "Nail mock ManageSieve".to_owned(),
            version: true,
            max_redirects: Some(4),
            max_script_size: None,
            max_scripts: None,
        }
    }
}

/// A script stored on the mock server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredScript {
    /// Script name.
    pub name: String,
    /// Script text, as uploaded.
    pub content: String,
    /// Whether it is the active script.
    pub active: bool,
}

/// One accepted `PUTSCRIPT`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SieveUpload {
    /// Script name.
    pub name: String,
    /// Script text.
    pub content: String,
}

#[derive(Debug, Clone)]
enum Fault {
    No(String),
    Disconnect,
}

#[derive(Debug, Default)]
struct State {
    scripts: BTreeMap<String, String>,
    active: Option<String>,
    uploads: Vec<SieveUpload>,
    checks: Vec<String>,
    rejections: Vec<(String, String)>,
}

#[derive(Debug, Default)]
struct Shared {
    state: Mutex<State>,
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

/// A running mock ManageSieve server on `127.0.0.1`. Stops when dropped.
#[derive(Debug)]
pub struct MockSieveServer {
    addr: SocketAddr,
    ca_der: Option<Vec<u8>>,
    shared: Arc<Shared>,
    shutdown: watch::Sender<bool>,
    task: JoinHandle<()>,
}

impl MockSieveServer {
    /// Starts a server without scripts on a free loopback port.
    pub async fn start(config: MockSieveConfig) -> Result<Self, MockNetError> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let addr = listener.local_addr()?;
        let tls = match config.tls {
            MockTls::None => None,
            MockTls::Implicit | MockTls::StartTls => Some(TestTls::generate()?),
        };
        let ca_der = tls.as_ref().map(|t| t.ca_der.clone());
        let acceptor = tls.map(|t| t.acceptor);
        let shared = Arc::new(Shared::default());
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
                                tracing::debug!(%err, "mock ManageSieve session ended");
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

    /// Stores a script directly (e.g. a script the user wrote elsewhere),
    /// optionally making it the active one. Not validated, not recorded as
    /// an upload.
    pub fn set_script(&self, name: &str, content: &str, active: bool) {
        let mut state = lock(&self.shared.state);
        state.scripts.insert(name.to_owned(), content.to_owned());
        if active {
            state.active = Some(name.to_owned());
        }
    }

    /// Every stored script, by name.
    pub fn scripts(&self) -> Vec<StoredScript> {
        let state = lock(&self.shared.state);
        state
            .scripts
            .iter()
            .map(|(name, content)| StoredScript {
                name: name.clone(),
                content: content.clone(),
                active: state.active.as_deref() == Some(name.as_str()),
            })
            .collect()
    }

    /// The text of one stored script.
    pub fn script(&self, name: &str) -> Option<String> {
        lock(&self.shared.state).scripts.get(name).cloned()
    }

    /// The name of the active script.
    pub fn active(&self) -> Option<String> {
        lock(&self.shared.state).active.clone()
    }

    /// Every accepted `PUTSCRIPT`, in order.
    pub fn uploads(&self) -> Vec<SieveUpload> {
        lock(&self.shared.state).uploads.clone()
    }

    /// The text of every `CHECKSCRIPT`, in order (accepted or not).
    pub fn checks(&self) -> Vec<String> {
        lock(&self.shared.state).checks.clone()
    }

    /// Every command received, in order: the command word and its quoted
    /// arguments; literals are shown as `{n}` and SASL payloads are
    /// removed.
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

    /// Answers the next `times` occurrences of `command` with `NO "text"`.
    pub fn fail(&self, command: &str, text: &str, times: usize) {
        lock(&self.shared.faults).push((
            command.to_ascii_uppercase(),
            Fault::No(text.to_owned()),
            times,
        ));
    }

    /// Drops the connection without answering the next `command`.
    pub fn disconnect_on(&self, command: &str) {
        lock(&self.shared.faults).push((command.to_ascii_uppercase(), Fault::Disconnect, 1));
    }

    /// Refuses `PUTSCRIPT` / `CHECKSCRIPT` of every script containing
    /// `needle`, answering `NO "message"`.
    pub fn reject_scripts_containing(&self, needle: &str, message: &str) {
        lock(&self.shared.state)
            .rejections
            .push((needle.to_owned(), message.to_owned()));
    }
}

impl Drop for MockSieveServer {
    fn drop(&mut self) {
        let _ = self.shutdown.send(true);
        self.task.abort();
    }
}

// ---------------------------------------------------------------------------
// Protocol

/// A token of a client command line.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    Atom(String),
    Str(Vec<u8>),
}

impl Tok {
    fn text(&self) -> String {
        match self {
            Self::Atom(a) => a.clone(),
            Self::Str(s) => String::from_utf8_lossy(s).into_owned(),
        }
    }
}

/// Splits one line into tokens; a trailing `{n}` / `{n+}` literal marker is
/// returned separately.
fn tokenize(line: &str) -> Result<(Vec<Tok>, Option<usize>), &'static str> {
    let bytes = line.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' => i += 1,
            b'"' => {
                i += 1;
                let mut s = Vec::new();
                loop {
                    match bytes.get(i) {
                        None => return Err("unterminated string"),
                        Some(b'"') => {
                            i += 1;
                            break;
                        }
                        Some(b'\\') => {
                            let Some(c) = bytes.get(i + 1) else {
                                return Err("unterminated string");
                            };
                            s.push(*c);
                            i += 2;
                        }
                        Some(c) => {
                            s.push(*c);
                            i += 1;
                        }
                    }
                }
                out.push(Tok::Str(s));
            }
            b'{' => {
                let end = line[i..].find('}').ok_or("unterminated literal")? + i;
                if end + 1 != bytes.len() {
                    return Err("literal marker must end the line");
                }
                let n = line[i + 1..end].trim_end_matches('+');
                let n: usize = n.parse().map_err(|_| "invalid literal length")?;
                return Ok((out, Some(n)));
            }
            _ => {
                let start = i;
                while i < bytes.len() && !matches!(bytes[i], b' ' | b'\t' | b'"' | b'{') {
                    i += 1;
                }
                out.push(Tok::Atom(line[start..i].to_owned()));
            }
        }
    }
    Ok((out, None))
}

/// Reads one command: a line plus any literals it announces.
async fn read_command(conn: &mut Connection) -> std::io::Result<Option<Result<Vec<Tok>, String>>> {
    let mut tokens = Vec::new();
    loop {
        let Some(raw) = conn.read_line().await? else {
            return Ok(None);
        };
        let line = String::from_utf8_lossy(&raw).into_owned();
        match tokenize(&line) {
            Err(e) => return Ok(Some(Err(e.to_owned()))),
            Ok((mut toks, None)) => {
                tokens.append(&mut toks);
                return Ok(Some(Ok(tokens)));
            }
            Ok((mut toks, Some(n))) => {
                tokens.append(&mut toks);
                if n > 16 * 1024 * 1024 {
                    return Ok(Some(Err("literal too large".to_owned())));
                }
                tokens.push(Tok::Str(conn.read_exact_bytes(n).await?));
            }
        }
    }
}

/// A server string: quoted when possible, else a literal.
fn string(text: &str) -> String {
    if text.contains(['\r', '\n']) || text.len() > 1000 {
        format!("{{{}}}\r\n{text}", text.len())
    } else {
        format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

fn capabilities(config: &MockSieveConfig, tls: bool) -> String {
    let tls_ok = config.tls != MockTls::StartTls || tls;
    let mut lines = vec![
        format!("\"IMPLEMENTATION\" {}", string(&config.implementation)),
        format!(
            "\"SASL\" \"{}\"",
            if tls_ok {
                config.sasl.join(" ")
            } else {
                String::new()
            }
        ),
        format!("\"SIEVE\" \"{}\"", config.extensions.join(" ")),
    ];
    if config.tls == MockTls::StartTls && !tls {
        lines.push("\"STARTTLS\"".to_owned());
    }
    if let Some(max) = config.max_redirects {
        lines.push(format!("\"MAXREDIRECTS\" \"{max}\""));
    }
    if config.version {
        lines.push("\"VERSION\" \"1.0\"".to_owned());
    }
    lines.push("OK \"Nail mock ManageSieve ready\"".to_owned());
    format!("{}\r\n", lines.join("\r\n"))
}

/// The command as logged: the verb and its arguments, literals as `{n}`,
/// no SASL payloads.
fn log_line(tokens: &[Tok]) -> String {
    let verb = tokens.first().map(Tok::text).unwrap_or_default();
    if verb.eq_ignore_ascii_case("AUTHENTICATE") {
        return format!(
            "AUTHENTICATE {}",
            tokens.get(1).map(Tok::text).unwrap_or_default()
        );
    }
    let mut out = verb.to_ascii_uppercase();
    for t in &tokens[1..] {
        out.push(' ');
        match t {
            Tok::Atom(a) => out.push_str(a),
            Tok::Str(s) if s.contains(&b'\n') || s.len() > 200 => {
                let _ = write!(out, "{{{}}}", s.len());
            }
            Tok::Str(s) => {
                let _ = write!(out, "\"{}\"", String::from_utf8_lossy(s));
            }
        }
    }
    out
}

async fn session(
    tcp: TcpStream,
    shared: &Shared,
    config: &MockSieveConfig,
    acceptor: Option<&TlsAcceptor>,
) -> std::io::Result<()> {
    drop(tcp.set_nodelay(true));
    let mut conn = match (config.tls, acceptor) {
        (MockTls::Implicit, Some(acceptor)) => Connection::accept_tls(tcp, acceptor).await?,
        _ => Connection::new(MockStream::Plain(tcp)),
    };
    conn.write(capabilities(config, conn.is_tls()).as_bytes())
        .await?;
    let mut authenticated = false;
    loop {
        let tokens = match read_command(&mut conn).await? {
            None => return Ok(()),
            Some(Err(e)) => {
                conn.line(&format!("NO {}", string(&e))).await?;
                continue;
            }
            Some(Ok(t)) if t.is_empty() => continue,
            Some(Ok(t)) => t,
        };
        lock(&shared.commands).push(log_line(&tokens));
        let verb = tokens[0].text().to_ascii_uppercase();
        let args: Vec<String> = tokens[1..].iter().map(Tok::text).collect();
        match shared.take_fault(&verb) {
            Some(Fault::Disconnect) => return Ok(()),
            Some(Fault::No(text)) => {
                conn.line(&format!("NO {}", string(&text))).await?;
                continue;
            }
            None => {}
        }
        let tls_ok = config.tls != MockTls::StartTls || conn.is_tls();
        match verb.as_str() {
            "CAPABILITY" => {
                conn.write(capabilities(config, conn.is_tls()).as_bytes())
                    .await?;
            }
            "STARTTLS" => {
                match acceptor.filter(|_| config.tls == MockTls::StartTls && !conn.is_tls()) {
                    Some(acceptor) => {
                        conn.line("OK \"Begin TLS negotiation now\"").await?;
                        conn = conn.upgrade(acceptor).await?;
                        conn.write(capabilities(config, true).as_bytes()).await?;
                    }
                    None => conn.line("NO \"STARTTLS not available\"").await?,
                }
            }
            "LOGOUT" => {
                conn.line("OK \"Logout completed\"").await?;
                conn.shutdown().await;
                return Ok(());
            }
            "NOOP" if config.version => conn.line("OK \"Done\"").await?,
            "AUTHENTICATE" => {
                if authenticated {
                    conn.line("NO \"Already authenticated\"").await?;
                } else if !tls_ok {
                    conn.line("NO (ENCRYPT-NEEDED) \"STARTTLS first\"").await?;
                } else if sasl_auth(&mut conn, config, &args).await? {
                    authenticated = true;
                    shared.logins.fetch_add(1, Ordering::SeqCst);
                    conn.line("OK \"Logged in\"").await?;
                }
            }
            _ if !authenticated => {
                conn.line("NO \"Authenticate first\"").await?;
            }
            _ => {
                let reply = script_command(shared, config, &verb, &args);
                conn.write(reply.as_bytes()).await?;
            }
        }
    }
}

/// Runs an `AUTHENTICATE` exchange; `Ok(true)` when logged in (the caller
/// sends `OK`), `Ok(false)` after a `NO` was sent.
async fn sasl_auth(
    conn: &mut Connection,
    config: &MockSieveConfig,
    args: &[String],
) -> std::io::Result<bool> {
    let mechanism = args
        .first()
        .cloned()
        .unwrap_or_default()
        .to_ascii_uppercase();
    if !config.sasl.contains(&mechanism) {
        conn.line("NO \"Unsupported mechanism\"").await?;
        return Ok(false);
    }
    let payload = if let Some(initial) = args.get(1) {
        initial.clone()
    } else {
        conn.line("\"\"").await?;
        match read_command(conn).await? {
            Some(Ok(t)) => t.first().map(Tok::text).unwrap_or_default(),
            _ => String::new(),
        }
    };
    if payload.trim() == "*" {
        conn.line("NO \"Authentication cancelled\"").await?;
        return Ok(false);
    }
    let Some(decoded) = sasl::b64_decode(&payload) else {
        conn.line("NO \"Invalid base64\"").await?;
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
            conn.line(&format!("\"{}\"", sasl::oauth_error_challenge()))
                .await?;
            drop(read_command(conn).await?);
        }
        ok
    };
    if !ok {
        conn.line("NO \"Authentication failed\"").await?;
    }
    Ok(ok)
}

/// Answers a script command of an authenticated session (full reply text,
/// CRLF-terminated).
fn script_command(
    shared: &Shared,
    config: &MockSieveConfig,
    verb: &str,
    args: &[String],
) -> String {
    let mut state = lock(&shared.state);
    let ok = |text: &str| format!("OK \"{text}\"\r\n");
    let no = |text: &str| format!("NO {}\r\n", string(text));
    let arg = |i: usize| args.get(i).map(String::as_str);
    match (verb, arg(0), arg(1)) {
        ("LISTSCRIPTS", _, _) => {
            let mut out = String::new();
            for name in state.scripts.keys() {
                out.push_str(&string(name));
                if state.active.as_deref() == Some(name.as_str()) {
                    out.push_str(" ACTIVE");
                }
                out.push_str("\r\n");
            }
            out + &ok("Listscripts completed")
        }
        ("PUTSCRIPT", Some(name), Some(content)) => {
            if let Some(max) = config.max_script_size
                && content.len() > max
            {
                return format!("NO (QUOTA/MAXSIZE) \"Script is larger than {max} bytes\"\r\n");
            }
            if let Some(max) = config.max_scripts
                && !state.scripts.contains_key(name)
                && state.scripts.len() >= max
            {
                return "NO (QUOTA/MAXSCRIPTS) \"Too many scripts\"\r\n".to_owned();
            }
            if name.is_empty() {
                return no("Empty script name");
            }
            if let Err(e) = check(&state, config, content) {
                return no(&e);
            }
            state.scripts.insert(name.to_owned(), content.to_owned());
            state.uploads.push(SieveUpload {
                name: name.to_owned(),
                content: content.to_owned(),
            });
            ok("Putscript completed")
        }
        ("CHECKSCRIPT", Some(content), _) if config.version => {
            state.checks.push(content.to_owned());
            match check(&state, config, content) {
                Ok(()) => ok("Script is valid"),
                Err(e) => no(&e),
            }
        }
        ("GETSCRIPT", Some(name), _) => match state.scripts.get(name) {
            Some(content) => format!(
                "{{{}}}\r\n{content}\r\n{}",
                content.len(),
                ok("Getscript completed")
            ),
            None => "NO (NONEXISTENT) \"Script does not exist\"\r\n".to_owned(),
        },
        ("SETACTIVE", Some(""), _) => {
            state.active = None;
            ok("No script is active")
        }
        ("SETACTIVE", Some(name), _) => {
            if state.scripts.contains_key(name) {
                state.active = Some(name.to_owned());
                ok("Setactive completed")
            } else {
                "NO (NONEXISTENT) \"Script does not exist\"\r\n".to_owned()
            }
        }
        ("DELETESCRIPT", Some(name), _) => {
            if state.active.as_deref() == Some(name) {
                "NO (ACTIVE) \"The active script cannot be deleted\"\r\n".to_owned()
            } else if state.scripts.remove(name).is_some() {
                ok("Deletescript completed")
            } else {
                "NO (NONEXISTENT) \"Script does not exist\"\r\n".to_owned()
            }
        }
        ("RENAMESCRIPT", Some(old), Some(new)) if config.version => {
            if state.scripts.contains_key(new) {
                return "NO (ALREADYEXISTS) \"Target exists\"\r\n".to_owned();
            }
            let Some(content) = state.scripts.remove(old) else {
                return "NO (NONEXISTENT) \"Script does not exist\"\r\n".to_owned();
            };
            state.scripts.insert(new.to_owned(), content);
            if state.active.as_deref() == Some(old) {
                state.active = Some(new.to_owned());
            }
            ok("Renamescript completed")
        }
        ("HAVESPACE", Some(_), Some(size)) => {
            let size: usize = size.parse().unwrap_or(usize::MAX);
            match config.max_script_size {
                Some(max) if size > max => {
                    format!("NO (QUOTA/MAXSIZE) \"Script is larger than {max} bytes\"\r\n")
                }
                _ => ok("Putscript would succeed"),
            }
        }
        _ => no("Unknown command or wrong arguments"),
    }
}

fn check(state: &State, config: &MockSieveConfig, content: &str) -> Result<(), String> {
    if let Some((_, message)) = state
        .rejections
        .iter()
        .find(|(needle, _)| content.contains(needle.as_str()))
    {
        return Err(message.clone());
    }
    validate_script(content, &config.extensions)
}

// ---------------------------------------------------------------------------
// Minimal script validation

/// Core commands and tests of RFC 5228 (no `require` needed).
const CORE: &[&str] = &[
    "require", "if", "elsif", "else", "stop", "keep", "discard", "redirect", "allof", "anyof",
    "not", "true", "false", "header", "address", "exists", "size",
];

/// Identifiers and tags that need an extension.
const NEEDS: &[(&str, &str)] = &[
    ("fileinto", "fileinto"),
    ("reject", "reject"),
    ("envelope", "envelope"),
    ("vacation", "vacation"),
    ("addflag", "imap4flags"),
    ("setflag", "imap4flags"),
    ("removeflag", "imap4flags"),
    ("hasflag", "imap4flags"),
    (":flags", "imap4flags"),
    (":copy", "copy"),
    ("set", "variables"),
    ("string", "variables"),
    ("body", "body"),
    (":regex", "regex"),
    (":value", "relational"),
    (":count", "relational"),
    ("date", "date"),
    ("currentdate", "date"),
    (":index", "index"),
    (":last", "index"),
    ("include", "include"),
    ("return", "include"),
    ("global", "include"),
    (":create", "mailbox"),
    ("mailboxexists", "mailbox"),
];

#[derive(Debug, PartialEq)]
enum Lex {
    Ident(String),
    Tag(String),
    Number,
    Str(String),
    Punct(char),
}

fn lex(script: &str) -> Result<Vec<(usize, Lex)>, String> {
    let chars: Vec<char> = script.chars().collect();
    let mut out = Vec::new();
    let mut line = 1usize;
    let mut i = 0usize;
    let err = |line: usize, text: &str| Err(format!("line {line}: {text}"));
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\n' => {
                line += 1;
                i += 1;
            }
            c if c.is_whitespace() => i += 1,
            '#' => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '/' if chars.get(i + 1) == Some(&'*') => {
                i += 2;
                loop {
                    if i + 1 >= chars.len() {
                        return err(line, "unterminated comment");
                    }
                    if chars[i] == '*' && chars[i + 1] == '/' {
                        i += 2;
                        break;
                    }
                    if chars[i] == '\n' {
                        line += 1;
                    }
                    i += 1;
                }
            }
            '"' => {
                let start = line;
                i += 1;
                let mut s = String::new();
                loop {
                    match chars.get(i) {
                        None => return err(start, "unterminated string"),
                        Some('"') => {
                            i += 1;
                            break;
                        }
                        Some('\\') => {
                            if let Some(n) = chars.get(i + 1) {
                                s.push(*n);
                            }
                            i += 2;
                        }
                        Some(ch) => {
                            if *ch == '\n' {
                                line += 1;
                            }
                            s.push(*ch);
                            i += 1;
                        }
                    }
                }
                out.push((start, Lex::Str(s)));
            }
            '[' | ']' | '(' | ')' | '{' | '}' | ',' | ';' => {
                out.push((line, Lex::Punct(c)));
                i += 1;
            }
            ':' => {
                let start = i;
                i += 1;
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                if i == start + 1 {
                    return err(line, "empty tag");
                }
                let tag: String = chars[start..i].iter().collect();
                out.push((line, Lex::Tag(tag.to_ascii_lowercase())));
            }
            c if c.is_ascii_digit() => {
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
                if i < chars.len() && matches!(chars[i], 'K' | 'M' | 'G' | 'k' | 'm' | 'g') {
                    i += 1;
                }
                out.push((line, Lex::Number));
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let word: String = chars[start..i]
                    .iter()
                    .collect::<String>()
                    .to_ascii_lowercase();
                if word == "text" && chars.get(i) == Some(&':') {
                    // Multi-line string: rest of the line, then lines up to ".".
                    let start_line = line;
                    i += 1;
                    while i < chars.len() && chars[i] != '\n' {
                        i += 1;
                    }
                    let mut s = String::new();
                    loop {
                        if i >= chars.len() {
                            return err(start_line, "unterminated multi-line string");
                        }
                        // chars[i] == '\n': start of a new line.
                        i += 1;
                        line += 1;
                        let begin = i;
                        while i < chars.len() && chars[i] != '\n' {
                            i += 1;
                        }
                        let text: String = chars[begin..i].iter().collect();
                        let text = text.strip_suffix('\r').unwrap_or(&text);
                        if text == "." {
                            break;
                        }
                        s.push_str(text.strip_prefix('.').unwrap_or(text));
                        s.push('\n');
                    }
                    out.push((start_line, Lex::Str(s)));
                } else {
                    out.push((line, Lex::Ident(word)));
                }
            }
            other => return err(line, &format!("unexpected character {other:?}")),
        }
    }
    Ok(out)
}

/// Checks a script the way the mock server does before storing it:
/// lexical syntax, balanced brackets, statements ending in `;` or a block,
/// `require` only at the start and only for advertised `extensions`, known
/// commands, and extension commands / tags used only when required.
/// The error text names the line, like real servers.
pub fn validate_script(script: &str, extensions: &[String]) -> Result<(), String> {
    let tokens = lex(script)?;
    let available: BTreeSet<String> = extensions.iter().map(|e| e.to_ascii_lowercase()).collect();
    let mut required = BTreeSet::new();
    let mut stack: Vec<(usize, char)> = Vec::new();
    let mut at_command_start = true;
    let mut requires_allowed = true;
    let mut in_require = false;
    let mut comparator_next = false;
    for (index, (line, token)) in tokens.iter().enumerate() {
        let line = *line;
        let err = |text: String| Err(format!("line {line}: {text}"));
        if comparator_next {
            comparator_next = false;
            if let Lex::Str(name) = token {
                let name = name.to_ascii_lowercase();
                if name != "i;octet" && name != "i;ascii-casemap" {
                    let ext = format!("comparator-{name}");
                    if !required.contains(&ext) {
                        return err(format!("comparator {name:?} used without require"));
                    }
                }
            }
        }
        match token {
            Lex::Ident(word) => {
                if at_command_start && word == "require" {
                    if !requires_allowed {
                        return err("require must come before other commands".to_owned());
                    }
                    in_require = true;
                } else if at_command_start && stack.is_empty() {
                    requires_allowed = false;
                }
                at_command_start = false;
                if let Some((_, ext)) = NEEDS.iter().find(|(w, _)| w == word) {
                    if !required.contains(*ext) {
                        return err(format!("{word} used without require \"{ext}\""));
                    }
                } else if !CORE.contains(&word.as_str()) {
                    return err(format!("unknown command or test {word:?}"));
                }
            }
            Lex::Tag(tag) => {
                if tag == ":comparator" {
                    comparator_next = true;
                }
                if let Some((_, ext)) = NEEDS.iter().find(|(w, _)| w == tag)
                    && !required.contains(*ext)
                {
                    return err(format!("{tag} used without require \"{ext}\""));
                }
            }
            Lex::Str(s) if in_require => {
                let ext = s.to_ascii_lowercase();
                if !available.contains(&ext) {
                    return err(format!("unsupported extension {ext:?}"));
                }
                required.insert(ext);
            }
            Lex::Punct(p @ ('[' | '(' | '{')) => {
                stack.push((line, *p));
                if *p == '{' {
                    at_command_start = true;
                }
            }
            Lex::Punct(p @ (']' | ')' | '}')) => {
                let open = match p {
                    ']' => '[',
                    ')' => '(',
                    _ => '{',
                };
                match stack.pop() {
                    Some((_, o)) if o == open => {}
                    _ => return err(format!("unbalanced {p:?}")),
                }
                if *p == '}' {
                    at_command_start = true;
                }
            }
            Lex::Punct(';') => {
                if stack.last().is_some_and(|(_, open)| *open != '{') {
                    return err("unexpected \";\" inside brackets".to_owned());
                }
                at_command_start = true;
                in_require = false;
            }
            Lex::Str(_) | Lex::Number | Lex::Punct(_) => {}
        }
        if index + 1 == tokens.len() && !matches!(token, Lex::Punct(';' | '}')) {
            return err("missing \";\" at the end of the script".to_owned());
        }
    }
    if let Some((line, open)) = stack.last() {
        return Err(format!("line {line}: {open:?} is never closed"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sasl::b64_encode;
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

    fn exts() -> Vec<String> {
        DEFAULT_EXTENSIONS.iter().map(|e| (*e).to_owned()).collect()
    }

    #[test]
    fn validation_accepts_good_scripts() {
        let script = "# nail\r\nrequire [\"fileinto\", \"imap4flags\"];\r\n\
                      if header :contains \"subject\" \"x\" {\r\n  addflag \"\\\\Seen\";\r\n  fileinto \"Lists\";\r\n}\r\n";
        assert_eq!(validate_script(script, &exts()), Ok(()));
        let vacation = "require \"vacation\";\nvacation :days 7 text:\nAway.\n..dot\n.\n;\n";
        assert_eq!(validate_script(vacation, &exts()), Ok(()));
        assert_eq!(validate_script("", &exts()), Ok(()));
        assert_eq!(validate_script("/* c */ keep;", &exts()), Ok(()));
        let unicode = "require \"comparator-i;unicode-casemap\";\n\
                       if header :comparator \"i;unicode-casemap\" :is \"subject\" \"ä\" { keep; }\n";
        assert_eq!(validate_script(unicode, &exts()), Ok(()));
    }

    #[test]
    fn validation_rejects_bad_scripts() {
        let bad = [
            ("fileinto \"x\";", "without require"),
            ("require \"nope\";", "unsupported extension"),
            ("keep", "missing"),
            ("if true { keep;", "never closed"),
            ("if true { keep; ]", "unbalanced"),
            ("keep; require \"fileinto\";", "must come before"),
            ("frobnicate;", "unknown command"),
            ("keep \"unterminated;", "unterminated string"),
            (
                "if header :regex \"a\" \"b\" { keep; }",
                ":regex used without",
            ),
            (
                "if header :comparator \"i;unicode-casemap\" :is \"a\" \"b\" { keep; }",
                "comparator",
            ),
            (
                "require \"vacation\";\nvacation text:\nno end\n",
                "multi-line",
            ),
            ("keep; /* open", "comment"),
            ("keep; @", "unexpected character"),
        ];
        for (script, needle) in bad {
            let err = validate_script(script, &exts()).unwrap_err();
            assert!(err.contains(needle), "{script:?}: {err}");
            assert!(err.starts_with("line "), "{err}");
        }
        let err = validate_script("require \"fileinto\";\n\nfileinto;\nkeep", &exts()).unwrap_err();
        assert!(err.starts_with("line 4:"), "{err}");
        // An extension the server does not advertise.
        let only_fileinto = vec!["fileinto".to_owned()];
        assert!(validate_script("require \"vacation\";", &only_fileinto).is_err());
    }

    #[test]
    fn tokenizes_commands() {
        assert_eq!(
            tokenize("PUTSCRIPT \"a \\\"b\\\"\" {12+}").unwrap(),
            (
                vec![Tok::Atom("PUTSCRIPT".into()), Tok::Str(b"a \"b\"".to_vec())],
                Some(12)
            )
        );
        assert_eq!(tokenize("X {3}").unwrap().1, Some(3));
        assert!(tokenize("X \"open").is_err());
        assert!(tokenize("X {3} y").is_err());
        assert!(tokenize("X {z}").is_err());
        assert_eq!(
            log_line(&[
                Tok::Atom("authenticate".into()),
                Tok::Str(b"PLAIN".to_vec()),
                Tok::Str(b"c2VjcmV0".to_vec())
            ]),
            "AUTHENTICATE PLAIN"
        );
        assert_eq!(
            log_line(&[
                Tok::Atom("putscript".into()),
                Tok::Str(b"n".to_vec()),
                Tok::Str(b"a\nb".to_vec())
            ]),
            "PUTSCRIPT \"n\" {3}"
        );
        assert_eq!(string("a\"b\\"), r#""a\"b\\""#);
        assert_eq!(string("a\nb"), "{3}\r\na\nb");
    }

    /// A raw client for protocol-level tests.
    struct Raw {
        reader: BufReader<TcpStream>,
    }

    impl Raw {
        async fn connect(server: &MockSieveServer) -> Self {
            let tcp = TcpStream::connect(server.addr()).await.unwrap();
            let mut raw = Self {
                reader: BufReader::new(tcp),
            };
            raw.until_status().await;
            raw
        }

        async fn line(&mut self) -> String {
            let mut line = String::new();
            self.reader.read_line(&mut line).await.unwrap();
            line.trim_end().to_owned()
        }

        /// Reads lines up to the status line; returns (data lines, status).
        async fn until_status(&mut self) -> (Vec<String>, String) {
            let mut lines = Vec::new();
            loop {
                let line = self.line().await;
                if line.starts_with("OK") || line.starts_with("NO") || line.starts_with("BYE") {
                    return (lines, line);
                }
                if let Some(n) = line
                    .strip_suffix('}')
                    .and_then(|l| l.rsplit_once('{'))
                    .and_then(|(_, n)| n.parse::<usize>().ok())
                {
                    let mut data = vec![0; n];
                    self.reader.read_exact(&mut data).await.unwrap();
                    lines.push(String::from_utf8(data).unwrap());
                    // The rest of the line after the literal (empty here).
                    assert_eq!(self.line().await, "");
                    continue;
                }
                lines.push(line);
            }
        }

        async fn send(&mut self, text: &str) -> (Vec<String>, String) {
            self.reader
                .get_mut()
                .write_all(format!("{text}\r\n").as_bytes())
                .await
                .unwrap();
            self.until_status().await
        }

        async fn login(&mut self) {
            let payload = b64_encode(b"\0sam@nail.example\0secret");
            let (_, status) = self
                .send(&format!("AUTHENTICATE \"PLAIN\" \"{payload}\""))
                .await;
            assert!(status.starts_with("OK"), "{status}");
        }
    }

    #[tokio::test]
    async fn greeting_and_script_lifecycle() {
        let server = MockSieveServer::start(MockSieveConfig::default())
            .await
            .unwrap();
        let tcp = TcpStream::connect(server.addr()).await.unwrap();
        let mut raw = Raw {
            reader: BufReader::new(tcp),
        };
        let (caps, status) = raw.until_status().await;
        assert!(status.starts_with("OK"));
        assert!(caps.iter().any(|l| l.starts_with("\"SIEVE\" \"fileinto")));
        assert!(caps.contains(&"\"VERSION\" \"1.0\"".to_owned()));
        assert!(caps.contains(&"\"MAXREDIRECTS\" \"4\"".to_owned()));

        let (_, status) = raw.send("LISTSCRIPTS").await;
        assert!(status.contains("Authenticate first"));
        raw.login().await;
        assert_eq!(server.logins(), 1);

        let script = "require \"fileinto\";\r\nfileinto \"A\";\r\n";
        let (_, status) = raw
            .send(&format!(
                "PUTSCRIPT \"nail\" {{{}+}}\r\n{script}",
                script.len()
            ))
            .await;
        assert!(status.starts_with("OK"), "{status}");
        let (_, status) = raw.send("SETACTIVE \"nail\"").await;
        assert!(status.starts_with("OK"));
        server.set_script("mine", "keep;", false);
        let (lines, _) = raw.send("LISTSCRIPTS").await;
        assert_eq!(lines, ["\"mine\"", "\"nail\" ACTIVE"]);
        let (lines, status) = raw.send("GETSCRIPT \"nail\"").await;
        assert!(status.starts_with("OK"));
        assert_eq!(lines, [script]);
        let (_, status) = raw.send("DELETESCRIPT \"nail\"").await;
        assert!(status.starts_with("NO (ACTIVE)"));
        let (_, status) = raw.send("GETSCRIPT \"gone\"").await;
        assert!(status.starts_with("NO (NONEXISTENT)"));
        let (_, status) = raw.send("RENAMESCRIPT \"mine\" \"theirs\"").await;
        assert!(status.starts_with("OK"));
        let (_, status) = raw.send("SETACTIVE \"\"").await;
        assert!(status.starts_with("OK"));
        let (_, status) = raw.send("DELETESCRIPT \"nail\"").await;
        assert!(status.starts_with("OK"));
        assert_eq!(server.active(), None);
        assert_eq!(
            server.scripts(),
            [StoredScript {
                name: "theirs".into(),
                content: "keep;".into(),
                active: false
            }]
        );
        assert_eq!(server.uploads().len(), 1);
        assert_eq!(server.uploads()[0].content, script);

        // CHECKSCRIPT validates without storing.
        let bad = "fileinto \"x\";";
        let (_, status) = raw
            .send(&format!("CHECKSCRIPT {{{}+}}\r\n{bad}", bad.len()))
            .await;
        assert!(
            status.starts_with("NO \"line 1: fileinto used without require \\\"fileinto\\\"\""),
            "{status}"
        );
        assert_eq!(server.checks(), [bad]);
        let (_, status) = raw.send("HAVESPACE \"x\" 100").await;
        assert!(status.starts_with("OK"));
        let (_, status) = raw.send("NOOP").await;
        assert!(status.starts_with("OK"));
        let (_, status) = raw.send("LOGOUT").await;
        assert!(status.starts_with("OK"));
        assert!(server.commands().contains(&"AUTHENTICATE PLAIN".to_owned()));
        assert!(
            server
                .commands()
                .iter()
                .any(|c| c.starts_with("PUTSCRIPT \"nail\" {"))
        );
    }

    #[tokio::test]
    async fn faults_quotas_and_rejections() {
        let server = MockSieveServer::start(MockSieveConfig {
            max_script_size: Some(20),
            max_scripts: Some(1),
            ..MockSieveConfig::default()
        })
        .await
        .unwrap();
        let mut raw = Raw::connect(&server).await;
        raw.login().await;
        let big = "keep;".repeat(10);
        let (_, status) = raw
            .send(&format!("PUTSCRIPT \"a\" {{{}+}}\r\n{big}", big.len()))
            .await;
        assert!(status.starts_with("NO (QUOTA/MAXSIZE)"));
        let (_, status) = raw.send("HAVESPACE \"a\" 100").await;
        assert!(status.starts_with("NO (QUOTA/MAXSIZE)"));
        let (_, status) = raw.send("PUTSCRIPT \"a\" \"keep;\"").await;
        assert!(status.starts_with("OK"));
        let (_, status) = raw.send("PUTSCRIPT \"b\" \"keep;\"").await;
        assert!(status.starts_with("NO (QUOTA/MAXSCRIPTS)"));

        server.reject_scripts_containing("discard", "discard is not allowed here");
        let (_, status) = raw.send("PUTSCRIPT \"a\" \"discard;\"").await;
        assert_eq!(status, "NO \"discard is not allowed here\"");
        server.fail("setactive", "try later", 1);
        let (_, status) = raw.send("SETACTIVE \"a\"").await;
        assert_eq!(status, "NO \"try later\"");
        let (_, status) = raw.send("SETACTIVE \"a\"").await;
        assert!(status.starts_with("OK"));
        server.disconnect_on("LISTSCRIPTS");
        raw.reader
            .get_mut()
            .write_all(b"LISTSCRIPTS\r\n")
            .await
            .unwrap();
        let mut rest = String::new();
        assert_eq!(raw.reader.read_line(&mut rest).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn sasl_variants() {
        let server = MockSieveServer::start(MockSieveConfig {
            oauth_tokens: vec![("sam@nail.example".into(), "tok".into())],
            ..MockSieveConfig::default()
        })
        .await
        .unwrap();
        // PLAIN without initial response.
        let mut raw = Raw::connect(&server).await;
        raw.reader
            .get_mut()
            .write_all(b"AUTHENTICATE \"PLAIN\"\r\n")
            .await
            .unwrap();
        assert_eq!(raw.line().await, "\"\"");
        let payload = b64_encode(b"\0sam@nail.example\0secret");
        let (_, status) = raw.send(&format!("\"{payload}\"")).await;
        assert!(status.starts_with("OK"), "{status}");
        // Wrong password, cancelled exchange, unknown mechanism.
        let mut raw = Raw::connect(&server).await;
        let bad = b64_encode(b"\0sam@nail.example\0nope");
        let (_, status) = raw.send(&format!("AUTHENTICATE \"PLAIN\" \"{bad}\"")).await;
        assert!(status.starts_with("NO"));
        let (_, status) = raw.send("AUTHENTICATE \"CRAM-MD5\"").await;
        assert!(status.starts_with("NO"));
        raw.reader
            .get_mut()
            .write_all(b"AUTHENTICATE \"PLAIN\"\r\n")
            .await
            .unwrap();
        assert_eq!(raw.line().await, "\"\"");
        let (_, status) = raw.send("\"*\"").await;
        assert!(status.contains("cancelled"));
        // XOAUTH2: good token, then a bad one with the error challenge.
        let good = b64_encode(b"user=sam@nail.example\x01auth=Bearer tok\x01\x01");
        let (_, status) = raw
            .send(&format!("AUTHENTICATE \"XOAUTH2\" \"{good}\""))
            .await;
        assert!(status.starts_with("OK"));
        let mut raw = Raw::connect(&server).await;
        let wrong = b64_encode(b"user=sam@nail.example\x01auth=Bearer old\x01\x01");
        raw.reader
            .get_mut()
            .write_all(format!("AUTHENTICATE \"XOAUTH2\" \"{wrong}\"\r\n").as_bytes())
            .await
            .unwrap();
        assert!(raw.line().await.starts_with("\"eyJ"));
        let (_, status) = raw.send("\"\"").await;
        assert!(status.starts_with("NO"));
        assert_eq!(server.logins(), 2);
    }

    #[tokio::test]
    async fn starttls_hides_sasl_until_encrypted() {
        let server = MockSieveServer::start(MockSieveConfig {
            tls: MockTls::StartTls,
            ..MockSieveConfig::default()
        })
        .await
        .unwrap();
        assert!(server.ca_der().is_some());
        let tcp = TcpStream::connect(server.addr()).await.unwrap();
        let mut raw = Raw {
            reader: BufReader::new(tcp),
        };
        let (caps, _) = raw.until_status().await;
        assert!(caps.contains(&"\"SASL\" \"\"".to_owned()));
        assert!(caps.contains(&"\"STARTTLS\"".to_owned()));
        let payload = b64_encode(b"\0sam@nail.example\0secret");
        let (_, status) = raw
            .send(&format!("AUTHENTICATE \"PLAIN\" \"{payload}\""))
            .await;
        assert!(status.starts_with("NO (ENCRYPT-NEEDED)"));
    }
}
